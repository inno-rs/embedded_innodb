use std::{
    cmp::PartialEq,
    sync::{Arc, Mutex},
};

use intrusive_collections::LinkedList;

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
    pub fn new(
        dblwr: Arc<Mutex<DoubleWriteBuffer>>,
        fil: Arc<Mutex<Fil>>,
        redo_log: Arc<Mutex<Log>>,
    ) -> Self {
        Self { dblwr, flush_list: Default::default(), fil, redo_log }
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
    pub fn batch(
        &mut self,
        flush_type: BufFlushType,
        min_n: usize,
        lsn_limit: Lsn,
        neighborhood: usize,
    ) -> usize {
        ut_ad!(flush_type == BufFlushList || flush_type == BufFlushLru);
        // The number of pages flushed.
        let page_count = 0;

        while page_count < min_n {
            let page_id = Option::<PageId>::None;

            if flush_type == BufFlushList {
                let cursor = self.flush_list.back_mut();
            }
        }

        page_count
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
    pub fn free_margin(&mut self, n_to_flush: usize) {
        if n_to_flush == 0 {
            return;
        }
        let n_flushed = self.batch(BufFlushLru, n_to_flush, 0, 0);
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
        unimplemented!()
    }
}
