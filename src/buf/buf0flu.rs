use crate::buf::buf0buf::BufPoolInner;
use intrusive_collections::LinkedList;
use std::sync::RwLockWriteGuard;
use std::{
    cmp::PartialEq,
    sync::{Arc, Mutex},
};
use tracing::error;

use crate::buf::buf0lru::BufLRU;
use crate::buf::buf0types::BufPage;
use crate::{
    buf::{
        buf0dblwr::DoubleWriteBuffer,
        buf0flu::BufFlushType::{BufFlushList, BufFlushLru, BufFlushNTypes},
        buf0types::{
            BUF_NO_CHECKSUM_MAGIC, BufBlock, BufIoFix, BufPageState,
            FlushListAdapter,
        },
    },
    fil::{
        fil0fil::Fil,
        fil0types::{
            FIL_PAGE_END_LSN_CHKSUM_RANGE, FIL_PAGE_LSN_RANGE,
            FIL_PAGE_SPACE_OR_CHKSUM_RANGE,
        },
    },
    fsp::fsp0types::SYS_TABLESPACE,
    innodb::innodb0types::{
        Lsn, PageFrame, PageId, RawPage, SpaceId, UNIV_PAGE_SIZE,
    },
    log::log0log::{Log, LogWaitMode},
    os::os0file::IORequest,
    page::page0page::Page,
    some_or_panic,
    trx::trx0sys::TRX_SYS_SPACE,
    ut::ut0dbg::{ut_a, ut_ad},
    ut_panic,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BufFlushType {
    /// Flush via the lru list.
    BufFlushLru = 0,
    /// Flush a single page.
    BufFlushSinglePage,
    /// Flush via the flush list.
    BufFlushList,
    /// ??
    BufFlushNTypes,
    None,
}

pub struct BufFlush {
    /// Double write buffer.
    dblwr: Arc<Mutex<DoubleWriteBuffer>>,
    /// List of buffer blocks to flush.
    flush_list: LinkedList<FlushListAdapter>,
    /// File interface for I/O operations.
    fil: Arc<Mutex<Fil>>,
    /// Redo Log
    redo_log: Arc<Mutex<Log>>,

    /// NEW: single-thread gate (one flag per type). This might
    /// be needed later when we have multi thread code for flushing
    in_flight: [bool; BufFlush::N_TYPES],
}

impl BufFlush {
    pub const N_TYPES: usize = BufFlushNTypes as usize;

    /// Creates a new BufFlush instance with the specified components.
    ///
    /// # Arguments
    /// * `dblwr` - Reference to the double write buffer
    /// * `fil` - Reference to the file interface for I/O operations
    /// * `redo_log` - Reference to the redo log
    ///
    /// # Returns
    /// A new BufFlush instance with an empty flush list
    ///

    #[inline]
    fn idx(t: BufFlushType) -> usize {
        match t {
            BufFlushLru => 0,
            BufFlushList => 2, // matches your enum values
            _ => panic!("unsupported flush type {:?}", t),
        }
    }

    pub fn new(
        dblwr: Arc<Mutex<DoubleWriteBuffer>>,
        fil: Arc<Mutex<Fil>>,
        redo_log: Arc<Mutex<Log>>,
    ) -> Self {
        Self {
            dblwr,
            flush_list: Default::default(),
            fil,
            redo_log,
            in_flight: [false; BufFlush::N_TYPES],
        }
    }

    /// Returns the number of pages in the flush list.
    ///
    /// # Returns
    /// The count of pages currently in the flush list
    pub fn get_flush_list_size(&self) -> usize {
        self.flush_list.iter().count()
    }

    /// Flushes a buffer block to disk according to the specified flush type.
    ///
    /// This method prepares the buffer block for writing by setting its I/O fix
    /// and flush type, then delegates to write_block_low for the actual I/O operation.
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block to flush
    /// * `flush_type` - The type of flush operation to perform (LRU or List)
    ///
    /// # Panic
    /// Panics if:
    /// - The buffer block is not in the BufBlockFilePage state
    /// - The flush type is not BufFlushLru or BufFlushList
    pub fn flush(
        &mut self,
        buf_block: &mut BufBlock,
        flush_type: BufFlushType,
    ) {
        ut_ad!(
            buf_block.buf_block_get_state() == BufPageState::BufBlockFilePage
        );
        ut_ad!(flush_type == BufFlushLru || flush_type == BufFlushList);

        {
            let buf_page = buf_block.buf_block_get_page_mut();

            ut_ad!(buf_page.in_file());

            buf_page.buf_page_set_io_fix(BufIoFix::BufIoWrite);
            buf_page.buf_page_set_flush_type(flush_type);
        }

        match flush_type {
            BufFlushList | BufFlushLru => {
                buf_block.frame_s_latch();
            }
            _rest_of_the_types => {
                ut_panic!(
                    "buf flush type should never be {:?} or {:?}",
                    BufFlushList,
                    BufFlushLru
                );
            }
        }

        self.write_block_low(buf_block);
    }

    /// Flushes a batch of buffer blocks from the end of the flush list or LRU list.
    ///
    /// This method attempts to flush a specified minimum number of pages from either
    /// the flush list or the LRU list, depending on the flush type. It processes pages
    /// in batches for better I/O efficiency, and can limit the flush operation to pages
    /// with a log sequence number (LSN) below a certain threshold.
    ///
    /// # Arguments
    /// * `flush_type` - The type of flush operation (BufFlushList or BufFlushLru)
    /// * `min_n` - The minimum number of pages to flush
    /// * `lsn_limit` - Only flush pages with LSN less than this value
    /// * `neighborhood` - The size of the neighborhood to scan for contiguous pages
    ///
    /// # Returns
    /// * `usize` - The number of pages actually flushed
    ///
    /// # Panic
    /// Panics if the flush type is not BufFlushList or BufFlushLru
    /// Flush a batch from LRU or FLUSH-LIST.
    /// Returns usize::MAX if a same-type batch is already in flight (matches your C++ sentinel).
    /// Flush a batch of pages either from the flush-list or via LRU neighborhoods.
    /// Returns the number of pages actually flushed.
    ///
    /// NOTE: requires &mut BufPoolInner so we can scan the LRU and call `try_neighbors`.
    pub fn batch(
        &mut self,
        pool: &mut BufPoolInner,
        lru: &mut BufLRU,
        flush_type: BufFlushType,
        min_n: usize,
        _lsn_limit: Lsn,
        _neighborhood: usize,
    ) -> usize {
        if !self.start_batch(flush_type) {
            // Another batch of this type is "in-flight"
            return usize::MAX;
        }

        let mut flushed = 0usize;

        match flush_type {
            BufFlushList => {
                // Snapshot a *budget* from the tail; some may not be ready.
                let total = self.get_flush_list_size();
                let budget = total.min(min_n.saturating_mul(8)).max(min_n);

                let mut candidates = Vec::with_capacity(budget);
                {
                    for r in self.flush_list.iter().rev().take(budget) {
                        candidates.push(r.clone()); // UnsafeRef<BufPage>-like handle
                    }
                } // release borrow of flush_list

                for page_ref in candidates {
                    if let Some(block_arc) =
                        page_ref.buf_page_get_buf_block().upgrade()
                    {
                        let mut block = block_arc.write().unwrap();
                        if self.ready_for_flush(&block, flush_type) {
                            self.flush(&mut block, flush_type);
                            flushed += 1;
                            if flushed >= min_n {
                                break;
                            }
                        }
                    }
                }
            }

            BufFlushLru => {
                // Collect anchor PageIds from the cold end of the LRU, then for each anchor,
                // flush its neighborhood via `try_neighbors`, until we reach `min_n`.
                let curr = pool.curr_size().max(1);
                let read_ahead_area = pool.get_read_ahead_area(); // your port of ut_2_power_up(m_curr_size/32).min(64)
                let scan_len = 5 + 2 * read_ahead_area; // same formula as C++

                // Snapshot anchor PageIds so we don’t hold a borrow while flushing
                let anchors: Vec<_> = {
                    // Immutable access is enough to iterate
                    lru.lru_list()
                        .iter()
                        .rev()
                        .take(scan_len)
                        .map(|bp| bp.get_page_id())
                        .collect()
                };

                for pid in anchors {
                    // NB: try_neighbors internally checks age rules and fix counts,
                    // and returns how many it actually flushed in that neighborhood.
                    let n = self.try_neighbors(
                        pool,
                        pid.space_id,
                        pid.page_no,
                        lru.get_lru_size(),
                        BufFlushLru,
                    );
                    flushed += n;
                    if flushed >= min_n {
                        break;
                    }
                }
            }

            _ => { /* unreachable in practice for batch() */ }
        }

        self.end_batch(flush_type);
        flushed
    }
    /// Checks if a buffer block is modified and ready for flushing.
    ///
    /// This method determines whether a buffer block can be flushed to disk based on
    /// its current state and the requested flush type. A block is ready for flushing if:
    /// 1. It has been modified (has a non-zero oldest modification LSN)
    /// 2. It is not currently involved in an I/O operation
    /// 3. For LRU flushes, it must also have no buffer fixes
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block to check
    /// * `flush_type` - The type of flush operation (BufFlushList or BufFlushLru)
    ///
    /// # Returns
    /// * `true` - If the block is ready for flushing
    /// * `false` - If the block cannot be flushed at this time
    ///
    /// # Panic
    /// Panics if:
    /// - The buffer page is not in a file
    /// - The flush type is not BufFlushList or BufFlushLru
    /// - For list flushes, if the page is not in the flush list
    pub fn ready_for_flush(
        &self,
        buf_block: &BufBlock,
        flush_type: BufFlushType,
    ) -> bool {
        let buf_page_ref = buf_block.buf_block_get_page();
        ut_ad!(buf_page_ref.in_file());
        ut_ad!(flush_type == BufFlushList || flush_type == BufFlushLru);

        if buf_page_ref.get_oldest_modification() != 0
            && buf_page_ref.buf_page_get_io_fix() == BufIoFix::BufIoNone
        {
            if flush_type != BufFlushLru {
                ut_ad!(buf_page_ref.is_in_flush_list());

                return true;
            } else if buf_page_ref.buf_page_io_fix_count() == 0 {
                return true;
            }
        }

        false
    }

    /// Performs the low-level write operation for a buffer block.
    ///
    /// This method handles the actual process of writing a buffer block to disk:
    /// 1. Forces the redo log up to the page's newest modification LSN
    /// 2. Initializes the block for writing
    /// 3. Posts the block to the double write buffer
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block to write
    ///
    /// # Panic
    /// Panics if:
    /// - The buffer page is not in a file
    /// - The buffer page's I/O fix is not set to BufIoWrite
    /// - The buffer page has no modifications (oldest or newest modification LSN is 0)
    /// - The buffer page is not in the BufBlockFilePage state
    fn write_block_low(&mut self, buf_block: &mut BufBlock) {
        let buf_page_ref = buf_block.buf_block_get_page_mut();

        ut_ad!(buf_page_ref.in_file());
        ut_ad!(buf_page_ref.buf_page_get_io_fix() == BufIoFix::BufIoWrite);
        ut_ad!(buf_page_ref.get_oldest_modification() != 0);
        ut_ad!(buf_page_ref.get_newest_modification() != 0);

        // Force the log before writing the modified block.
        {
            let mut log = self.redo_log.lock().unwrap();
            log.write_up_to(
                buf_page_ref.get_newest_modification(),
                LogWaitMode::LogWait,
                true,
            );
        }

        match buf_page_ref.get_state() {
            BufPageState::BufBlockFilePage => {
                let mut page_frame = RawPage::new();
                BufFlush::init_for_writing(
                    &mut page_frame.as_page_frame(),
                    buf_page_ref.get_newest_modification(),
                );
            }
            _rest_of_the_types => {
                ut_panic!(
                    "unexpected buf page state: {:?}",
                    _rest_of_the_types
                );
            }
        }

        // Post the block to the double write buffer.
        self.post_to_double_write_buf(buf_block);
    }

    /// Initializes the buffer block for writing.
    ///
    /// This method prepares a buffer block before it is written to disk.
    /// Currently, this is a placeholder for future implementation.
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block to initialize
    /// * `new_lsn` - The newest log sequence number (LSN) for this block
    pub fn init_for_writing(page_frame: &mut PageFrame, new_lsn: Lsn) {
        // Write the newest modification LSN to the page header and trailer.
        page_frame.0[FIL_PAGE_LSN_RANGE]
            .copy_from_slice(&new_lsn.to_le_bytes());
        page_frame.0[FIL_PAGE_END_LSN_CHKSUM_RANGE]
            .copy_from_slice(&new_lsn.to_le_bytes());

        // Store the new formula checksum.
        let checksum = Page::page_data_calc_checksum(page_frame);
        page_frame.0[FIL_PAGE_SPACE_OR_CHKSUM_RANGE]
            .copy_from_slice(&checksum.to_le_bytes());

        // We overwrite the first 4 bytes of the end lsn field to store the old formula checksum.
        // Since it depends also on the field FIL_PAGE_SPACE_OR_CHKSUM,
        // we need to write it after the checksum.
        page_frame.0[FIL_PAGE_END_LSN_CHKSUM_RANGE]
            .copy_from_slice(&BUF_NO_CHECKSUM_MAGIC.to_le_bytes());
    }

    /// Posts a buffer block to the double write buffer.
    ///
    /// This method attempts to append the buffer block to the double write buffer,
    /// retrying if the operation fails. It continues trying until the operation
    /// succeeds.
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block to post to the double write buffer
    ///
    /// # Panic
    /// Panics if the buffer page is not in the BufBlockFilePage state
    pub fn post_to_double_write_buf(&mut self, buf_block: &mut BufBlock) {
        let mut continue_writing = true;
        while continue_writing {
            let buf_page_ref = buf_block.buf_block_get_page_mut();
            ut_a!(buf_page_ref.get_state() == BufPageState::BufBlockFilePage);

            let mut dblwr = self.dblwr.lock().unwrap();

            match dblwr.append(buf_block) {
                Ok(_) => {
                    tracing::info!(
                        "successfully posted page to double write buffer"
                    );
                    continue_writing = false;
                }
                Err(err) => {
                    tracing::error!(err);
                    continue_writing = true;
                }
            }
        }
    }

    /// Flushes buffered writes from the double write buffer to disk.
    ///
    /// This method processes all modified blocks in the double write buffer:
    /// 1. Writes each block to the system tablespace
    /// 2. Flushes the system tablespace to ensure durability
    /// 3. Clears the double write buffer
    ///
    /// This two-phase write process is critical for crash recovery. If the system
    /// crashes during a page write, the copy in the double write buffer can be
    /// used to recover the page.
    ///
    /// # Panic
    /// Panics if:
    /// - Failed to get a raw block from the double write buffer
    /// - Failed to write to the file
    pub fn buffered_writes(&mut self) {
        let mut dblwr = self.dblwr.lock().unwrap();

        if dblwr.is_empty() {
            return;
        }

        let mut fil = self.fil.lock().unwrap();

        dblwr.get_modified_blocks().iter().for_each(|block| {
            let space_id = block.space_id;
            let page_no = block.page_no;

            let raw_page = some_or_panic!(
                dblwr.get_raw_block(block),
                "Failed to get raw block from double write buffer"
            );

            fil.io(
                IORequest::AsyncWrite,
                false,
                TRX_SYS_SPACE as SpaceId,
                page_no,
                0,
                PageFrame::new(
                    raw_page
                        .as_mut()
                        .try_into()
                        .expect("should be able to convert to PageFrame"),
                )
                .0,
                UNIV_PAGE_SIZE,
            )
            .expect("should not fail to write to file");
        });

        // Flush the double write buffer to the file system.
        fil.flush(SYS_TABLESPACE as SpaceId);
        dblwr.set_empty();

        // Now we can start writing the pages to their corresponding data files.
        // If they are corrupted due to a crash,
        // they will be recovered from the double write buffer.
    }

    /// Frees up space in the buffer pool by flushing pages from the LRU list.
    ///
    /// This method attempts to flush a specified number of pages from the LRU (Least Recently Used)
    /// list to make room for new pages in the buffer pool. If a batch flush operation is already
    /// in progress (indicated by `batch` returning `usize::MAX`), it waits for that operation
    /// to complete before returning.
    ///
    /// # Arguments
    /// * `n_to_flush` - The number of pages to flush from the LRU list
    ///
    /// # Implementation Note
    /// This method uses the `batch` operation with `BufFlushLru` type to perform the actual
    /// flushing of pages. The LSN limit and neighborhood parameters are set to 0, meaning
    /// all eligible pages can be flushed regardless of their LSN.
    pub fn free_margin(&mut self, lru: &mut BufLRU, inner: &mut BufPoolInner) {
        let n_to_flush: usize = lru.recommendation();
        if n_to_flush == 0 {
            return;
        }

        let n_flushed = self.batch(inner, lru, BufFlushLru, n_to_flush, 0, 0);
        if n_flushed == usize::MAX {
            self.wait_batch_end(BufFlushLru);
        }
    }

    /// Waits for the completion of an in-flight batch flush operation.
    ///
    /// This method blocks the current thread until a batch flush operation of the specified
    /// type completes. It is called when the `batch` method returns `usize::MAX`, indicating
    /// that there is already a batch operation in progress for the given flush type.
    ///
    /// # Arguments
    /// * `flush_type` - The type of flush operation to wait for (BufFlushLru or BufFlushList)
    ///
    /// # Implementation Note
    /// This method should block on an OS-specific event or condition variable associated with
    /// the specified flush type. The implementation will depend on the threading and
    /// synchronization mechanisms used in the system.
    ///  TODO need to discuss how to implement this
    fn wait_batch_end(&self, flush_type: BufFlushType) {
        // block on your OS‐event or Condvar for that flush_type…
        // no op yet.
        // TODO implement this when we have concurrency.
    }

    /// Try to flush dirty neighbors near (space, page_no).
    ///
    /// Returns how many pages we flushed.
    ///
    /// Port of InnoDB:
    ///   - If LRU is small (< OLD_MIN_LENGTH): only try (page_no, page_no+1)
    ///   - Else: compute a neighborhood "area" and align [low, high) to that chunk
    ///   - Clamp `high` to the space size
    ///   - Walk each page in [low, high):
    ///       * lookup in page-hash
    ///       * if LRU-flush: skip non-old neighbors except the anchor page
    ///       * lock block; if ready_for_flush (and neighbor buf-fix==0), flush
    pub fn try_neighbors(
        &mut self,
        pool: &mut BufPoolInner,
        space: SpaceId,
        page_no: u32,
        lru_len: usize,
        flush_type: BufFlushType,
    ) -> usize {
        use crate::buf::buf0lru::OLD_MIN_LENGTH;

        ut_ad!(matches!(flush_type, BufFlushLru | BufFlushList));

        // 1) Compute [low, high)
        let (low, mut high) = if lru_len < OLD_MIN_LENGTH {
            // If the LRU is tiny (lru_len < OLD_MIN_LENGTH), don’t get fancy: just the anchor page [page_no, page_no+1).
            (page_no, page_no.saturating_add(1))
        } else {
            let curr = pool.curr_size().max(1);
            let area = pool.get_read_ahead_area().min(curr / 16).max(1);
            let base = (page_no / area as u32) * area as u32;
            (base, base + area as u32)
        };

        // Clamp to the actual space size
        let space_pages = { self.fil.lock().unwrap().space_get_size(space) };
        if (high as usize) > space_pages {
            high = space_pages as u32;
        }

        // 2) Walk the neighborhood
        let mut count = 0usize;
        for i in low..high {
            // Lookup block in the pool hash
            let pid = PageId { space_id: space, page_no: i };
            let Some(block_arc) = pool.page_hash().get(&pid).cloned() else {
                continue;
            };

            // Take the block write latch (single-thread path is fine to write-lock)
            let mut block = block_arc.write().unwrap();
            let page = block.buf_block_get_page();

            // the page must be file-backed
            ut_ad!(page.in_file());

            // In LRU batches, prefer “old” neighbors (except the anchor page itself)
            if matches!(flush_type, BufFlushLru) {
                if i != page_no && !page.is_old() {
                    continue;
                }
            }

            // Only flush neighbors (≠ anchor) when buf-fix count is zero
            let fix_ok = if i == page_no {
                true
            } else {
                page.buf_page_io_fix_count() == 0
            };

            if fix_ok && self.ready_for_flush(&block, flush_type) {
                self.flush(&mut block, flush_type);
                count += 1;
            }
        }

        count
    }

    /// If the page’s tablespace is truncated/deleted, free it immediately
    /// (no flush) and drop it from the flush-list if needed.
    fn free_page_if_truncated(
        &mut self,
        pool: &mut BufPoolInner,
        lru: &mut BufLRU,
        block: &mut RwLockWriteGuard<BufBlock>,
    ) -> bool {
        let pid = block.buf_block_get_page().get_page_id();

        // Ask FIL if this space is gone (or being deleted)
        let truncated = {
            let fil = self.fil.lock().unwrap();
            fil.tablespace_deleted_or_being_deleted(pid.space_id, -1)
        };
        if !truncated {
            return false;
        }

        // For logging/diagnostics
        error!(
            "freeing a page from truncated tablespace: space_id={}, page_no={}",
            pid.space_id, pid.page_no
        );

        // Was the page on the flush-list?
        let on_flush_list = block.buf_block_get_page().is_in_flush_list()
            || block.buf_block_get_page().get_oldest_modification() != 0;

        // Force-evict from LRU/hash → free-list
        {
            lru.free_block_truncated(pool, block);
        }

        // Also drop from the flush-list if it was there
        if on_flush_list {
            self.remove_from_flush_list(block.buf_block_get_page_mut());
        }

        true
    }

    #[inline]
    fn start_batch(&mut self, t: BufFlushType) -> bool {
        let i = Self::idx(t);
        if self.in_flight[i] {
            false
        } else {
            self.in_flight[i] = true;
            true
        }
    }

    #[inline]
    fn end_batch(&mut self, t: BufFlushType) {
        let i = Self::idx(t);
        self.in_flight[i] = false;
    }

    /// Remove `page` from the intrusive flush-list if it’s there.
    /// Returns `true` if a node was removed, `false` if the page wasn’t on the list.
    #[inline]
    pub fn remove_from_flush_list(&mut self, page: &mut BufPage) -> bool {
        // Optional: in debug builds, assert we only try to remove when flagged.
        // If you prefer to be lenient, keep this as a debug_assert! and still handle false.
        debug_assert!(page.is_in_flush_list());

        if !page.is_in_flush_list() {
            return false;
        }

        unsafe {
            // O(1) unlink using the node pointer embedded in `page`
            let mut cur = self.flush_list.cursor_mut_from_ptr(page);
            cur.remove();
        }

        page.set_is_in_flush_list(false);

        // If you keep an explicit length counter for the flush-list, decrement it here.
        // self.flush_list_len -= 1;

        true
    }

    pub fn flush_list(&mut self) -> &mut LinkedList<FlushListAdapter> {
        &mut self.flush_list
    }
}
