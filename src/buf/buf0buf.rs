use std::{
    alloc::{Layout, alloc_zeroed},
    collections::HashMap,
    ops::DerefMut,
    sync::{Arc, Mutex, RwLock, atomic::Ordering::Relaxed},
};

use intrusive_collections::{LinkedList, UnsafeRef};
use tracing::{error, info};

use crate::{
    buf::{
        buf0flu::{BufFlush, BufFlushType},
        buf0lru::BufLRU,
        buf0rea::BufRea,
        buf0types::{
            BUF_GET_ALWAYS, BUF_GET_IF_IN_POOL, BUF_GET_NO_LATCH, BufBlock,
            BufChunk, BufIoFix::BufIoRead, BufPage, BufPageState,
            BufPoolStats, FlushListAdapter, Request,
        },
    },
    fil::{
        fil0fil::Fil,
        fil0types::{
            FIL_PAGE_FILE_FLUSH_LSN, FIL_PAGE_NEXT, FIL_PAGE_PREV,
            FIL_PAGE_TYPE, FilPageType,
        },
    },
    innodb::innodb0types::{DbErr, PageId, PageNo, SpaceId, UNIV_PAGE_SIZE},
    mach::mach0data::{mach_write_to_2, mach_write_to_4, mach_write_to_8},
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{FixMode, MemoSlot, MtrState},
    },
    os::os0file::IORequest,
    some_or_panic,
    sync::sync0rw::ReadWriteLatch,
    ut::{
        ut0dbg::{ut_a, ut_ad},
        ut0ut::ut_time_ms,
    },
    ut_2_power_up, ut_panic,
};

/// Buffer Pool manages the caching of database pages in memory.
///
/// The buffer pool is a critical component of the database system that caches
/// frequently accessed pages to reduce disk I/O operations. It implements
/// various page replacement algorithms and manages the lifecycle of pages
/// in memory.
pub struct BufPool {
    /// Protects *exactly* the `inner` data above
    inner: Mutex<BufPoolInner>,
    /// File system interface for I/O operations
    fil: Arc<Mutex<Fil>>,
    /// reference to BufLru.
    buf_lru: Arc<Mutex<BufLRU>>,
    /// Statistics about the buffer pool
    stat: Arc<BufPoolStats>,
    /// Reference to the buffer read manager for handling page reads.
    buf_rea: Arc<Mutex<BufRea>>,
}

/// All the data that used to live *inside* the pool mutex
/// (i.e., everything protected by that lock) goes here.
pub(crate) struct BufPoolInner {
    /// A sequence number used to count pages evicted off the LRU list
    freed_page_clock: usize,
    /// Total number of frames in the pool
    curr_size: usize,
    /// In-memory hash of page_id → block
    page_hash: HashMap<PageId, Arc<RwLock<BufBlock>>>,
    /// List of modified pages that need to be flushed to disk
    flush_list: LinkedList<FlushListAdapter>,
    /// List of memory chunks allocated for the buffer pool
    chunks: Vec<Arc<RwLock<BufChunk>>>,
    /// When an LRU flush ends for a page, this is incremented by one;
    /// this is set back to zero whenever a new buffer block is allocated.
    lru_flush_ended: usize,
    /// true when a flush‐batch of that type is initializing
    init_flush: [bool; BufFlush::N_TYPES],
    /// how many write‐batches of each type are still pending
    n_flush: [usize; BufFlush::N_TYPES],
}

impl BufPoolInner {
    /// Construct an empty `BufPoolInner` for tests or benches.

    pub fn new() -> Self {
        BufPoolInner {
            freed_page_clock: 0,
            curr_size: 0,
            page_hash: HashMap::new(),
            flush_list: Default::default(),
            chunks: Vec::new(),
            lru_flush_ended: 0,
            init_flush: [false; BufFlush::N_TYPES],
            n_flush: [0; BufFlush::N_TYPES],
        }
    }
    /**

     Following methods are package protected and not Thread Safe
    // Should be called after getting buf pool inner lock.

     */
    pub(crate) fn freed_page_clock(&self) -> usize {
        self.freed_page_clock
    }

    pub(crate) fn curr_size(&self) -> usize {
        self.curr_size
    }

    pub(crate) fn page_hash(&self) -> &HashMap<PageId, Arc<RwLock<BufBlock>>> {
        &self.page_hash
    }

    #[cfg(test)]
    pub(crate) fn page_hash_mut(
        &mut self,
    ) -> &mut HashMap<PageId, Arc<RwLock<BufBlock>>> {
        &mut self.page_hash
    }

    pub(crate) fn flush_list(&self) -> &LinkedList<FlushListAdapter> {
        &self.flush_list
    }

    pub(crate) fn chunks(&self) -> &Vec<Arc<RwLock<BufChunk>>> {
        &self.chunks
    }

    pub(crate) fn lru_flush_ended(&self) -> usize {
        self.lru_flush_ended
    }

    /// Snapshot of the init-flush flags.
    pub(crate) fn init_flush(&self) -> [bool; BufFlush::N_TYPES] {
        self.init_flush
    }

    /// Snapshot of the pending-flush counters.
    pub(crate) fn n_flush(&self) -> [usize; BufFlush::N_TYPES] {
        self.n_flush
    }

    pub(crate) fn set_freed_page_clock(&mut self, freed_page_clock: usize) {
        self.freed_page_clock = freed_page_clock;
    }

    pub(crate) fn set_curr_size(&mut self, curr_size: usize) {
        self.curr_size = curr_size;
    }

    pub(crate) fn set_page_hash(
        &mut self,
        page_hash: HashMap<PageId, Arc<RwLock<BufBlock>>>,
    ) {
        self.page_hash = page_hash;
    }

    pub(crate) fn set_flush_list(
        &mut self,
        flush_list: LinkedList<FlushListAdapter>,
    ) {
        self.flush_list = flush_list;
    }

    pub(crate) fn set_chunks(&mut self, chunks: Vec<Arc<RwLock<BufChunk>>>) {
        self.chunks = chunks;
    }

    pub(crate) fn set_lru_flush_ended(&mut self, lru_flush_ended: usize) {
        self.lru_flush_ended = lru_flush_ended;
    }

    /// Overwrite all the “initializing” flags at once.
    pub(crate) fn set_init_flush(
        &mut self,
        init_flush: [bool; BufFlush::N_TYPES],
    ) {
        self.init_flush = init_flush;
    }

    /// Overwrite all the pending‐flush counters at once.
    pub(crate) fn set_n_flush(&mut self, n_flush: [usize; BufFlush::N_TYPES]) {
        self.n_flush = n_flush;
    }
}

impl BufPool {
    /// Creates a new Buffer Pool instance.
    ///
    /// # Parameters
    /// * `fil` - A reference to the file system interface for I/O operations
    ///
    /// # Returns
    /// A new BufPool instance with initialized data structures
    pub fn new(
        fil: Arc<Mutex<Fil>>,
        buf_lru: Arc<Mutex<BufLRU>>,
        buf_rea: Arc<Mutex<BufRea>>,
    ) -> Self {
        BufPool {
            inner: Mutex::new(BufPoolInner::new()),
            fil,
            buf_lru,
            stat: Arc::new(BufPoolStats::new()),
            buf_rea,
        }
    }

    /* Free List methods */
    /// Retrieves a free block from the buffer pool's free list.
    ///
    /// This method attempts to get an available buffer block from the free list
    /// for reuse. If successful, the block is removed from the free list.
    ///
    /// # Returns
    /// * `Some(BufBlock)` - A free buffer block if available
    /// * `None` - If no free blocks are available
    pub fn get_free_block(&self) -> Option<Arc<RwLock<BufBlock>>> {
        if let Some(r) = self.buf_lru.lock().unwrap().get_free_block() {
            info!("free block found");
            Some(r)
        } else {
            info!("free block not found");
            None
        }
    }

    /// Retrieves a buffer block for the requested page.
    ///
    /// This method attempts to find the requested page in the buffer pool.
    /// If the page is found, it returns a reference to the corresponding buffer block.
    /// If the page is not found and the request mode allows, it may load the page from disk.
    ///
    /// # Parameters
    /// * `request` - The request containing page ID, latch mode, and other parameters
    ///
    /// # Returns
    /// * `Some(&BufBlock)` - A reference to the buffer block containing the requested page
    /// * `None` - If the page is not in the pool and cannot be loaded based on request mode
    pub fn get(&mut self, request: Request) -> Option<Arc<RwLock<BufBlock>>> {
        ut_ad!(request.mtr.get_mtr_state() == MtrState::MtrActive);
        ut_ad!(
            request.rw_latch == ReadWriteLatch::NoLatch
                || request.rw_latch == ReadWriteLatch::SLatch
                || request.rw_latch == ReadWriteLatch::XLatch
        );
        ut_ad!(
            request.mode != BUF_GET_NO_LATCH
                || request.rw_latch == ReadWriteLatch::NoLatch
        );
        ut_ad!(
            request.mode == BUF_GET_ALWAYS
                || request.mode == BUF_GET_IF_IN_POOL
                || request.mode == BUF_GET_NO_LATCH
        );

        self.stat.n_pages_get.fetch_add(1, Relaxed);
        let mut n_retries = 0u32;

        // TODO(Rahul): Understand the logic of `guess`.

        // TODO: Implement retry logic for getting a page for n number of times.

        // 1. First, check if the page is already in the buffer pool:
        let block_opt = {
            // Lock the pool‐inner so we can safely inspect page_hash
            let inner = self.inner.lock().unwrap();
            inner.page_hash.get(&request.page_id).cloned()
        };

        match block_opt.clone() {
            // 2. The page is not in the buffer pool, and the request mode is BUF_GET_NO_LATCH.
            // Return None.
            None => {
                if request.mode == BUF_GET_IF_IN_POOL {
                    return None;
                }

                // 3. The page is not in the buffer pool, and try reading it.
                let result = self.init_for_read(
                    request.page_id.space_id,
                    request.page_id.page_no,
                    -1,
                );

                let b = match result {
                    Ok(Some(block)) => {
                        // Successfully initialized the page for read.
                        info!(
                            "page initialized for read: {}",
                            request.page_id
                        );
                        Some(block)
                    }
                    Ok(None) => {
                        ut_panic!(
                            "Failed to initialize page for read: Page not found in tablespace"
                        );
                    }
                    Err(err) => {
                        error!("Failed to initialize page for read: {}", err);
                        ut_panic!(
                            "Failed to initialize page for read: {}",
                            err
                        );
                    }
                };

                let buf_block =
                    some_or_panic!(b, "failed to initialize page for read");

                let buf_rea = self.buf_rea.lock().unwrap();
                if buf_rea.buf_read_page(
                    IORequest::SyncRead,
                    request.page_id.space_id,
                    request.page_id.page_no,
                    buf_block.clone(),
                ) {
                    // Successfully read the page.
                    let mut buf_block = buf_block.write().unwrap();
                    let buf_page_ref = buf_block.buf_block_get_page_mut();
                    self.io_complete(buf_page_ref);

                    n_retries += 1;
                }
            }
            Some(block) => {
                ut_ad!(
                    block.read().unwrap().get_page_id() == request.page_id,
                    "Block page_id={} does not match request page_id={}",
                    block.read().unwrap().get_page_id(),
                    request.page_id
                );
            }
        }

        let block =
            some_or_panic!(block_opt, "failed to get block from page hash");
        {
            let block_ref = block.read().unwrap();
            // The page is being read.
            let page_read_io =
                block_ref.buf_block_get_page().buf_page_get_io_fix()
                    == BufIoRead;

            if page_read_io && request.mode == BUF_GET_IF_IN_POOL {
                // If the page is being read, we cannot return it immediately.
                // We need to wait for the read operation to complete.
                return None;
            }
        }

        let mut block_guard = block.write().unwrap();
        let block_ref = block_guard.deref_mut();
        let state = block_ref.buf_block_get_state();
        match &state {
            BufPageState::BufBlockFilePage => {}
            _rest_of_the_types => {
                ut_panic!(
                    "Unexpected buffer block state: {:?}",
                    _rest_of_the_types
                );
            }
        }

        ut_ad!(state == BufPageState::BufBlockFilePage);
        // Increment the buffix count for the page.
        block_ref.buf_block_get_page().buf_page_io_fix_count();
        let buf_page = block_ref.buf_block_get_page_mut();
        let access_time = self.buf_page_accessed_time(buf_page);

        self.set_accessed_make_young(buf_page, access_time);

        match request.rw_latch {
            ReadWriteLatch::SLatch => {
                block_ref.frame_s_latch();
            }
            ReadWriteLatch::XLatch => {
                block_ref.frame_x_latch();
            }
            ReadWriteLatch::NoLatch => {
                // The caller does not need any latch.
            }
        }

        if access_time == 0 {
            // TODO: Implement logic for read ahead.
        }

        let fix_mode = match request.rw_latch {
            ReadWriteLatch::SLatch => FixMode::Shared,
            ReadWriteLatch::XLatch => FixMode::Exclusive,
            ReadWriteLatch::NoLatch => FixMode::NoLatch,
        };
        request.mtr.memo_push(MemoSlot::PageFix(request.page_id, fix_mode));

        Some(block.clone())
    }

    /// Calculates the size of the read-ahead area.
    ///
    /// This method determines the appropriate size for read-ahead operations
    /// based on the current buffer pool size. It returns the minimum of either
    /// the next power of 2 of (buffer pool size / 32) or 64.
    ///
    /// # Returns
    /// * `u64` - The calculated read-ahead area size
    pub fn get_read_ahead_area(&self) -> u64 {
        // Lock the inner state to read curr_size
        let inner = self.inner.lock().unwrap();
        let sz = inner.curr_size;

        // Use the same formula, converting to u64 as needed
        std::cmp::min(ut_2_power_up!(sz / 32) as u64, 64)
    }

    /// Retrieves a buffer block by its space ID and page number.
    ///
    /// This method looks up a page in the buffer pool's hash table using its
    /// space ID and page number.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    ///
    /// # Returns
    /// * `Some(&BufBlock)` - A reference to the buffer block if found
    /// * `None` - If the page is not in the buffer pool
    pub fn hash_get_page(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
    ) -> Option<Arc<RwLock<BufBlock>>> {
        let page_id = PageId { space_id, page_no };
        let inner = self.inner.lock().unwrap();
        let ret = inner.page_hash.get(&page_id).cloned();

        ut_ad!(
            ret.is_none()
                || ret.as_ref().unwrap().read().unwrap().get_page_id()
                    == page_id
        );

        ret
    }

    /// Removes a page from the buffer pool's hash table.
    ///
    /// This method is called when a page is being evicted from the buffer pool.
    /// It removes the mapping between the page ID and its buffer block from the
    /// hash table, making the page no longer accessible through the buffer pool.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    ///
    /// # Returns
    /// * `true` - If the page was found and removed from the hash table
    /// * `false` - If the page was not in the hash table
    /// Remove the given page from the pool’s hash (if present),
    /// clear its in‐hash flag under the block‐latch, and return true.
    /// This is a light weight method to remove a page from the hashmap.
    /// It does not remove the page from LRU and  do page eviction.
    pub fn hash_remove_page(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
    ) -> bool {
        let page_id = PageId { space_id, page_no };

        // 1) Pop it out of the hash under the pool lock
        let maybe_block = {
            let mut inner = self.inner.lock().unwrap();
            inner.page_hash.remove(&page_id)
        };

        // 2) If there was a block, grab its latch and clear the flag
        if let Some(block_arc) = maybe_block {
            let mut blk = block_arc.write().unwrap();
            let buf_page = blk.buf_block_get_page_mut();
            buf_page.set_is_in_hash_map(false);
            true
        } else {
            false
        }
    }

    /// Public API: remove a single page completely, evicting it from
    /// the LRU, un‐hashing it, and updating all clocks & stats.
    pub fn block_remove_hashed_page(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
    ) -> BufPageState {
        // 1) Lock the pool‐inner & LRU
        let mut inner = self.inner.lock().unwrap();
        let mut lru = self.buf_lru.lock().unwrap();

        // 2) Pull the BufBlock out of the page_hash (removing from the hash)
        let page_id = PageId { space_id, page_no };
        let block_arc = inner
            .page_hash
            .remove(&page_id)
            .unwrap_or_else(|| ut_panic!("page not in hash: {:?}", page_id));

        // 3) Lock the block for write & get the BufPage
        let mut block_guard = block_arc.write().unwrap();

        // 4) Let the LRU helper unlink it and bump the pool‐clock

        lru.remove_page_from_lru(&mut *inner, &mut block_guard);
        let buf_page = block_guard.buf_block_get_page_mut();
        // 5) Mark it out of the hash map
        buf_page.set_is_in_hash_map(false);

        let mut page = block_guard.get_page();
        page.page_set_space_id(0xFF);
        page.page_set_fil_offset(0xFF);

        block_guard.buf_block_set_state(BufPageState::BufBlockRemoveHash);
        ut_ad!(
            inner.page_hash.get(&page_id).is_none(),
            "evicted page still in hash: {:?}",
            page_id
        );
        BufPageState::BufBlockRemoveHash
    }

    /// Checks if a page is present in the buffer pool.
    ///
    /// This method verifies whether a specific page is currently cached
    /// in the buffer pool without retrieving the actual buffer block.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    ///
    /// # Returns
    /// * `true` - If the page is in the buffer pool
    /// * `false` - If the page is not in the buffer pool
    /// Returns true if the given page is currently in the buffer‐pool.
    pub fn peek(&self, space_id: SpaceId, page_no: PageNo) -> bool {
        let page_id = PageId { space_id, page_no };
        // Lock the inner pool state to access page_hash
        let inner = self.inner.lock().unwrap();
        inner.page_hash.contains_key(&page_id)
    }

    /// Handles the completion of an I/O operation on a buffer page.
    ///
    /// This method is called when an asynchronous I/O operation (read or write)
    /// on a buffer page completes. It updates the page state and performs
    /// any necessary post-processing.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page for which I/O has completed
    pub fn io_complete(&self, buf_page: &mut BufPage) {}

    /// Initializes a page for reading into the buffer pool.
    ///
    /// This method prepares a page read from the disk into the buffer pool.
    /// It checks if the page is already in the pool and handles the appropriate
    /// initialization based on the page's current state.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    /// * `table_space_version` - The version of the tablespace
    pub fn init_for_read(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
        table_space_version: i64,
    ) -> Result<Option<Arc<RwLock<BufBlock>>>, DbErr> {
        /* 1. Check if the block is present in the buffer pool.*/
        {
            let inner = self.inner.lock().unwrap();

            let block_get = inner.page_hash.get(&PageId { space_id, page_no });

            if let Some(buf_block) = block_get {
                return Ok(Some(buf_block.clone()));
            }
        }

        let block = self.get_free_block();

        /* 2. Check if the tablespace is being deleted or already deleted. */
        {
            let fil = self.fil.lock().unwrap();
            if fil.tablespace_deleted_or_being_deleted(
                space_id,
                table_space_version,
            ) {
                tracing::warn!(
                    "Trying to read a page from a deleted tablespace or is being deleted: {}",
                    space_id
                );
                return Ok(None);
            }
        }

        /* 3. Allocate a new buffer block for the page. */
        let block = some_or_panic!(block, "buf_block should be present");
        {
            let mut block_ref = block.write().unwrap();

            let mut buf_page_ref = block_ref.buf_block_get_page_mut();
            self.add_block(&mut buf_page_ref, false);
        }

        self.page_init(space_id, page_no, block.clone());

        let ret = Some(block.clone());
        Ok(ret)
    }

    /// Adds a buffer page to the flush list.
    ///
    /// This method adds a modified buffer page to the flush list, which tracks
    /// pages that need to be written back to disk. Pages in the flush list are
    /// ordered by their oldest modification LSN.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the flush list
    pub fn add_to_flush_list(&self, buf_page: &mut BufPage) {
        // 1) Push the node into the pool’s flush_list under the inner mutex
        let mut inner = self.inner.lock().unwrap();
        unsafe {
            inner.flush_list.push_back(UnsafeRef::from_raw(buf_page));
        }
        // inner lock drops here

        // 2) Mark the page’s flag (this still needs the block‐level latch)
        buf_page.set_is_in_flush_list(true);
    }

    /// Initializes a page in the buffer pool.
    ///
    /// This method sets up a buffer block to represent a specific page in a tablespace.
    /// It initializes the buffer page metadata with the provided space ID and page number,
    /// and prepares the block for use in the buffer pool.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    /// * `buf_block` - The buffer block to initialize
    pub fn page_init(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
        buf_block: Arc<RwLock<BufBlock>>,
    ) {
        info!(
            "Initializing page in buffer pool: space_id={}, page_no={}",
            space_id, page_no
        );

        // Sanity: this block must not already be marked as a file page
        let curr_state = buf_block.read().unwrap().buf_block_get_state();
        ut_ad!(curr_state != BufPageState::BufBlockFilePage);

        // Set the state of the block to BufBlockFilePage.
        // 1) Mark the block as a file page
        {
            let mut blk = buf_block.write().unwrap();
            // buf_block_set_file_page takes &mut BufBlock
            self.buf_block_set_file_page(&mut *blk, space_id, page_no);
        } // drop the block latch

        // 2) Insert into the pool’s hash under the pool‐inner mutex
        let mut inner = self.inner.lock().unwrap();
        let result = inner
            .page_hash
            .insert(PageId { space_id, page_no }, buf_block.clone());
        ut_ad!(result.is_none(), "failed to insert page into hash map");
    }

    /// Mark this BufBlock as holding a file‐page, and record its
    /// space_id/page_no in the BufPage.
    fn buf_block_set_file_page(
        &self,
        buf_block: &mut BufBlock,
        space_id: SpaceId,
        page_no: PageNo,
    ) {
        // 1) flip the state
        buf_block.buf_block_set_state(BufPageState::BufBlockFilePage);

        // 2) stash the identifiers on the BufPage
        let page = buf_block.buf_block_get_page_mut();
        page.set_space_id(space_id);
        page.set_page_no(page_no);

        // sanity check
        ut_ad!(page.in_file());
    }

    /// Frees a buffer block and returns it to the free list.
    ///
    /// This method releases a buffer block that is no longer needed, making it
    /// available for reuse. It delegates to the LRU manager's block_free_non_file_page
    /// method to handle the actual freeing process.
    ///
    /// # Parameters
    /// * `buf_block` - The buffer block to free
    pub fn block_free(&mut self, buf_block: Arc<RwLock<BufBlock>>) {
        let mut inner = self.inner.lock().unwrap();
        let mut guard = buf_block.write().unwrap();
        let mut lru = self.buf_lru.lock().unwrap();

        // 3) Sanity: must not be a file-backed page
        ut_a!(
            guard.buf_block_get_state() != BufPageState::BufBlockFilePage,
            "block_free called on file‐page"
        );

        lru.block_free_non_file_page(&mut *inner, &mut guard);
    }

    /// Creates a new page in the buffer pool for the given space_id and page_no.
    ///
    /// This function attempts to allocate a free block from the buffer pool and initialize it
    /// as a file page. If the page already exists in the buffer pool, it will reuse that page
    /// instead of creating a new one. The page is added to the LRU list as young and its access
    /// time is updated. The page is also pinned in the mini-transaction (mtr).
    ///
    /// # Arguments
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    /// * `mtr` - The mini-transaction that will use this page
    ///
    /// # Returns
    /// * `Some(Arc<RwLock<BufBlock>>)` - A reference to the created buffer block if successful
    /// * `None` - If no free block could be allocated from the buffer pool
    pub fn create(
        &mut self,
        space_id: SpaceId,
        page_no: PageNo,
        mtr: &mut Mtr,
    ) -> Option<Arc<RwLock<BufBlock>>> {
        ut_ad!(mtr.get_mtr_state() == MtrState::MtrActive);

        // 1) grab a free block
        let block = {
            let mut lru = self.buf_lru.lock().unwrap();
            lru.get_free_block()
        }?;

        // 2) if already present, evict the new one and re‐get
        if {
            let inner = self.inner.lock().unwrap();
            inner.page_hash.contains_key(&PageId { space_id, page_no })
        } {
            // clear the "was_freed" flag while holding the write lock,
            // then drop it before calling block_free()
            {
                let mut guard = block.write().unwrap();
                guard.buf_block_get_page_mut().set_file_page_was_freed(false);
            }
            self.block_free(block.clone()); // safe to grab block lock again
            return self.get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id, page_no },
                mode: BUF_GET_NO_LATCH,
                mtr,
            });
        }

        // 3) initialize as file‐page + hash insert
        self.page_init(space_id, page_no, block.clone());

        // 4) LRU insert as young
        {
            let mut guard = block.write().unwrap();
            let buf_page = guard.buf_block_get_page_mut();
            self.add_block(buf_page, false);
        }

        // 5) bump create‐stat
        self.stat.n_pages_created.fetch_add(1, Relaxed);

        // 6) timestamp & possible read‐ahead
        let now = ut_time_ms();
        {
            let mut guard = block.write().unwrap();
            let buf_page = guard.buf_block_get_page_mut();
            self.set_accessed_make_young(buf_page, now);
        }

        // 7) pin in MTR
        mtr.memo_push(MemoSlot::PageFix(
            PageId { space_id, page_no },
            FixMode::NoLatch,
        ));

        // 8) flush‐margin
        // let mut fl = self.flusher.lock().unwrap();
        // fl.free_margin();

        // 9) init on-disk header bytes
        {
            let guard = block.write().unwrap();
            let page = guard.get_page();
            let frame = page.to_page_frame();
            mach_write_to_4(
                &mut frame.0[FIL_PAGE_PREV..FIL_PAGE_PREV + 4],
                u32::MAX,
            );
            mach_write_to_4(
                &mut frame.0[FIL_PAGE_NEXT..FIL_PAGE_NEXT + 4],
                u32::MAX,
            );
            mach_write_to_2(
                &mut frame.0[FIL_PAGE_TYPE..FIL_PAGE_TYPE + 2],
                FilPageType::Allocated as u16,
            );
            mach_write_to_8(
                &mut frame.0
                    [FIL_PAGE_FILE_FLUSH_LSN..FIL_PAGE_FILE_FLUSH_LSN + 8],
                0,
            );
        }

        // 10) sanity
        ut_ad!(
            self.inner
                .lock()
                .unwrap()
                .page_hash
                .contains_key(&PageId { space_id, page_no })
        );

        Some(block)
    }

    /// Invalidates the entire buffer pool.
    ///
    /// This method performs a complete reset of the buffer pool by:
    /// 1) Waiting for any in-flight flush batches to finish
    /// 2) Draining out all buffer frames via LRU eviction
    /// 3) Resetting all LRU and pool state
    ///
    /// It's typically called during shutdown or when a major reconfiguration
    /// of the database is needed.
    pub fn invalidate(&mut self) {
        // ─── Phase 1: wait for any outstanding flushes ───

        // B) For each flush type from LRU .. N_TYPES
        for i in (BufFlushType::BufFlushLru as usize)
            ..(BufFlushType::BufFlushNTypes as usize)
        {
            // 1) At startup/recovery, no new write batches are initializing
            ut_ad!(
                !self.init_flush()[i],
                "invalidate(): init_flush[{}] must be false",
                i
            );

            // 2) But we may have an in-flight batch we posted earlier
            if self.n_flush()[i] > 0 {
                // drop the lock before waiting

                // wait for that batch to finish
                // TODO implement flusher
                //self.flusher.wait_batch_end(BufFlushType::from(i));
            }
            // end of scope: pool lock is dropped here
        }

        // ─── Phase 2: ensure everything’s freed ───

        // TODO implement all_freed()
        /* ut_ad!(
            self.all_freed(),
            "invalidate(): buffer pool must be empty before LRU eviction"
        );*/

        // Repeatedly free one LRU block at a time until none remain
        while self.search_and_free_block(100) {
            // spin
        }

        // ─── Phase 3: reset all state ───

        // After eviction, the LRU list *must* now be empty
        {
            let lru = self.buf_lru.lock().unwrap();
            ut_ad!(
                lru.lru_list().is_empty(),
                "invalidate(): LRU list must be empty after eviction"
            );
        }

        // Reset pool‐wide counters & pointers
        // 2) Clear pool‐wide counters & pointers

        self.set_freed_page_clock(0);
        self.set_lru_flush_ended(0);

        // 3) Clear all flush‐batch flags/counters
        self.set_init_flush([false; BufFlush::N_TYPES]);
        self.set_n_flush([0; BufFlush::N_TYPES]);

        // Reset the “old” markers in the LRU manager
        {
            let mut lru = self.buf_lru.lock().unwrap();
            lru.set_lru_old_head(None);
            lru.set_lru_old_len(0);
        }

        // Zero‐out stats
        self.stat = Arc::from(BufPoolStats::new());
        //   self.refresh_io_stats();

        // pool lock drops here
    }

    /// Returns the number of blocks in the free list.
    ///
    /// This method delegates to the LRU manager to get the current size of the free list,
    /// which indicates how many buffer blocks are available for immediate reuse.
    ///
    /// # Returns
    /// * `usize` - The number of blocks in the free list
    pub fn get_free_list_size(&self) -> usize {
        self.buf_lru.lock().unwrap().get_free_list_size()
    }

    /// Initializes a chunk of memory for the buffer pool.
    ///
    /// This method allocates a contiguous block of memory and divides it into
    /// fixed-size pages for use in the buffer pool. It creates buffer blocks for
    /// each page and adds them to the free list, making them available for use.
    ///
    /// # Parameters
    /// * `mem_size` - The total size of memory to allocate (must be a multiple of page size)
    ///
    /// # Returns
    /// * `Arc<RwLock<BufChunk>>` - A reference to the newly created buffer chunk
    ///
    /// # Panic
    /// Panics in debug mode if the memory size is not a multiple of the page size
    fn chunk_init(&mut self, mem_size: usize) -> Arc<RwLock<BufChunk>> {
        ut_ad!(mem_size % UNIV_PAGE_SIZE == 0);

        let layout =
            Layout::from_size_align(mem_size, UNIV_PAGE_SIZE).unwrap();
        let mem_ptr = unsafe { alloc_zeroed(layout) };

        let no_of_pages = mem_size / UNIV_PAGE_SIZE;

        let mut buf_pages =
            Vec::<Arc<RwLock<BufBlock>>>::with_capacity(no_of_pages);

        ut_ad!(buf_pages.len() == 0);

        let mut buf_frame_ptr = mem_ptr;
        for index in 0..no_of_pages {
            let buf_page = BufPage::new(0, index as PageNo);
            let buf_block =
                Arc::new(RwLock::new(BufBlock::new(buf_page, buf_frame_ptr)));

            let mut buf_block_guard = buf_block.write().unwrap();
            let buf_page_ref = buf_block_guard.buf_block_get_page_mut();

            self.buf_lru.lock().unwrap().add_to_free_list(buf_page_ref);

            let buf_block_weak = Arc::downgrade(&buf_block);

            buf_block_guard
                .buf_block_get_page_mut()
                .set_buf_block_ref(buf_block_weak.clone());

            buf_pages.push(buf_block.clone());

            unsafe {
                buf_frame_ptr = buf_frame_ptr.add(UNIV_PAGE_SIZE);
            }
        }

        ut_ad!(buf_pages.len() == no_of_pages);

        let rt = Arc::new(RwLock::new(BufChunk {
            layout,
            mem_size,
            size: no_of_pages,
            mem: mem_ptr,
            buf_pages,
        }));

        rt
    }

    /// Opens and initializes the buffer pool with the specified size.
    ///
    /// This method sets up the buffer pool by initializing a memory chunk of the
    /// specified size and adding it to the buffer pool's chunks. It updates the
    /// current size of the buffer pool to reflect the allocated memory.
    ///
    /// # Parameters
    /// * `pool_size` - The size of the buffer pool to initialize (in bytes)
    /// Allocate the initial chunk(s) and set the pool size.
    /// After this call returns, `current_size()` will return `pool_size`.
    pub fn open(&mut self, pool_size: usize) {
        // 1) Create the chunk (this can still use &self to access `fil`, etc.)
        let chunk = self.chunk_init(pool_size);

        // 2) Lock the inner state and update both fields in one critical section
        let mut inner = self.inner.lock().unwrap();
        inner.chunks.push(chunk);
        inner.curr_size = pool_size;
    }

    // ——————————————————————————————————————————————————
    // Page Access Methods
    // ——————————————————————————————————————————————————
    /// Sets the first access time of the page and potentially makes it young in the LRU list.
    ///
    /// Updates the access time of a buffer page and, if it's old enough,
    /// promotes it to the front of the LRU list to reduce the chance of eviction.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer block to update
    /// * `access_time` - The current access time to compare against the page's last access time
    pub fn set_accessed_make_young(
        &self,
        buf_page: &mut BufPage,
        access_time: u64,
    ) {
    }

    /// Gets the last access time of a buffer page.
    ///
    /// This method retrieves the timestamp of when the buffer page was last accessed.
    /// The access time is used to determine if a page should be made young in the LRU list.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to get the access time for
    ///
    /// # Returns
    /// * `u64` - The last access time of the buffer page
    pub fn buf_page_accessed_time(&self, buf_page: &BufPage) -> u64 {
        0
    }

    /// Increments the freed page counter.
    ///
    /// This method is called when a buffer block is removed from the LRU list
    /// and returned to the free list. It helps track how many pages have been
    /// freed during the buffer pool's lifetime.
    /// Increment the “freed page” clock counter.
    pub fn incr_freed_page_count(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.freed_page_clock += 1;
    }

    /// Searches for a buffer block to free and removes it from the LRU list.
    ///
    /// This method scans the LRU list for a buffer block that can be freed,
    /// and if found, removes it from the list and returns it to the free list.
    /// It performs a limited number of iterations to avoid excessive scanning.
    ///
    /// # Parameters
    /// * `n_iterations` - The maximum number of iterations to perform while searching
    ///
    /// # Returns
    /// * `bool` - `true` if a page was successfully freed, `false` otherwise
    pub fn search_and_free_block(&mut self, n_iterations: usize) -> bool {
        // 1) Lock the pool‐inner state
        let mut inner = self.inner.lock().unwrap();
        // 2) Lock the LRU manager
        let mut lru = self.buf_lru.lock().unwrap();
        // 3) Call the LRU’s private helper, passing the locked‐pool reference
        lru.search_and_free_block(&mut *inner, &self.stat, n_iterations)
        // both locks are dropped here
    }

    // ——————————————————————————————————————————————————
    // Component Setters
    // ——————————————————————————————————————————————————
    // ——————————————————————————————————————————————————
    // freed_page_clock
    // ——————————————————————————————————————————————————
    /// Gets the current value of the freed page clock.
    ///
    /// The freed page clock is a counter that increments each time a page is freed
    /// from the buffer pool. It's used to track the rate of page evictions and for
    /// various buffer pool management algorithms.
    ///
    /// # Returns
    /// * `usize` - The current value of the freed page clock
    pub fn freed_page_clock(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.freed_page_clock
    }

    /// Sets the value of the freed page clock.
    ///
    /// This method allows manually setting the freed page clock to a specific value,
    /// which is typically done during initialization or reset of the buffer pool.
    ///
    /// # Parameters
    /// * `v` - The new value for the freed page clock
    pub fn set_freed_page_clock(&self, v: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.freed_page_clock = v;
    }

    // ——————————————————————————————————————————————————
    // curr_size
    // ——————————————————————————————————————————————————
    /// Gets the current size of the buffer pool.
    ///
    /// This method returns the total size of the buffer pool in bytes,
    /// which represents the amount of memory allocated for caching database pages.
    ///
    /// # Returns
    /// * `usize` - The current size of the buffer pool in bytes
    pub fn current_size(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.curr_size
    }

    /// Gets the current size of the buffer pool.
    ///
    /// This method returns the total size of the buffer pool in bytes.
    ///
    /// # Returns
    /// * `usize` - The current size of the buffer pool in bytes
    pub fn get_curr_size(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.curr_size
    }

    /// Sets the current size of the buffer pool.
    ///
    /// This method allows manually setting the buffer pool size to a specific value,
    /// which is typically done during initialization or when resizing the buffer pool.
    ///
    /// # Parameters
    /// * `v` - The new size for the buffer pool in bytes
    pub fn set_current_size(&self, v: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.curr_size = v;
    }

    // ——————————————————————————————————————————————————
    // lru_flush_ended
    // ——————————————————————————————————————————————————
    /// Gets the LRU flush ended counter.
    ///
    /// This counter tracks the number of completed LRU flush operations.
    /// It's used to coordinate flushing activities and ensure proper synchronization
    /// between different components of the buffer pool.
    ///
    /// # Returns
    /// * `usize` - The current value of the LRU flush ended counter
    pub fn lru_flush_ended(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.lru_flush_ended
    }

    /// Sets the LRU flush ended counter.
    ///
    /// This method allows manually setting the LRU flush ended counter to a specific value,
    /// which is typically done during initialization or reset of the buffer pool.
    ///
    /// # Parameters
    /// * `v` - The new value for the LRU flush ended counter
    pub fn set_lru_flush_ended(&self, v: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.lru_flush_ended = v;
    }

    // ——————————————————————————————————————————————————
    // init_flush (snapshot copy)
    // ——————————————————————————————————————————————————
    /// Gets the initialization status of flush operations.
    ///
    /// This method returns an array indicating whether flush operations of each type
    /// are currently being initialized. It's used to coordinate flush operations
    /// and prevent concurrent initialization of the same flush type.
    ///
    /// # Returns
    /// * `[bool; BufFlush::N_TYPES]` - An array of boolean values, one for each flush type
    pub fn init_flush(&self) -> [bool; BufFlush::N_TYPES] {
        let inner = self.inner.lock().unwrap();
        inner.init_flush
    }

    /// Sets the initialization status of flush operations.
    ///
    /// This method allows setting the initialization status for all flush types at once.
    /// It's typically used during initialization or reset of the buffer pool.
    ///
    /// # Parameters
    /// * `arr` - An array of boolean values, one for each flush type
    pub fn set_init_flush(&self, arr: [bool; BufFlush::N_TYPES]) {
        let mut inner = self.inner.lock().unwrap();
        inner.init_flush = arr;
    }

    // ——————————————————————————————————————————————————
    // n_flush (snapshot copy)
    // ——————————————————————————————————————————————————
    /// Gets the number of pending flush operations of each type.
    ///
    /// This method returns an array containing the count of pending flush operations
    /// for each flush type. It's used to track and coordinate flush activities.
    ///
    /// # Returns
    /// * `[usize; BufFlush::N_TYPES]` - An array of counts, one for each flush type
    pub fn n_flush(&self) -> [usize; BufFlush::N_TYPES] {
        let inner = self.inner.lock().unwrap();
        inner.n_flush
    }

    /// Sets the number of pending flush operations of each type.
    ///
    /// This method allows setting the count of pending flush operations for all flush types
    /// at once. It's typically used during initialization or reset of the buffer pool.
    ///
    /// # Parameters
    /// * `arr` - An array of counts, one for each flush type
    pub fn set_n_flush(&self, arr: [usize; BufFlush::N_TYPES]) {
        let mut inner = self.inner.lock().unwrap();
        inner.n_flush = arr;
    }

    // ——————————————————————————————————————————————————
    // chunks (clone the Vec)
    // ——————————————————————————————————————————————————
    /// Gets the memory chunks allocated for the buffer pool.
    ///
    /// This method returns a vector of all memory chunks that have been allocated
    /// for the buffer pool. Each chunk represents a contiguous block of memory
    /// divided into fixed-size pages.
    ///
    /// # Returns
    /// * `Vec<Arc<RwLock<BufChunk>>>` - A vector of references to buffer chunks
    pub fn chunks(&self) -> Vec<Arc<RwLock<BufChunk>>> {
        let inner = self.inner.lock().unwrap();
        inner.chunks.clone()
    }

    /// Sets the memory chunks for the buffer pool.
    ///
    /// This method replaces the current set of memory chunks with a new set.
    /// It's typically used during initialization or when reconfiguring the buffer pool.
    ///
    /// # Parameters
    /// * `chunks` - A vector of references to buffer chunks
    pub fn set_chunks(&self, chunks: Vec<Arc<RwLock<BufChunk>>>) {
        let mut inner = self.inner.lock().unwrap();
        inner.chunks = chunks;
    }

    /// Adds a buffer page to the LRU list.
    ///
    /// This is a high-level method that adds a buffer page to the LRU list
    /// by delegating to the internal `add_block_low` method.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the LRU list
    pub fn add_block(&self, buf_page: &mut BufPage, old: bool) {
        // 1) Lock the pool‐inner if you ever need to update pool counters,
        //    or just to uphold the lock‐order discipline:
        let inner = self.inner.lock().unwrap();

        // 2) Lock the LRU manager
        let mut lru = self.buf_lru.lock().unwrap();

        // 3) Call the real insertion routine
        lru.add_block_low(buf_page, old);
    }

    /// Change the percent of pages treated as “old.”  If `adjust==true`,
    /// replay every flipped page through `get()`.  No LRU constants here.
    fn update_old_ratio(&mut self, old_pct: usize, adjust: bool) {
        // 1) Under the LRU lock, set the ratio and get the pages to touch.
        let changed: Vec<PageId> = {
            let mut lru = self.buf_lru.lock().unwrap();
            lru.set_old_ratio_pct_and_collect(old_pct)
        };

        // 2) If caller said “don’t adjust now” or nothing moved, bail
        if !adjust || changed.is_empty() {
            return;
        }

        // 3) Otherwise, replay each page through get(), using one MTR
        let mut mtr = Mtr::new();
        for pid in changed {
            let req = Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: pid,
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            };
            let _ = self.get(req);
        }
    }

    /// Test‐only: get a handle to the inner‐pool lock so we can
    /// inspect or manipulate BufPoolInner.
    ///  Will try to eliminate this.
    #[cfg(test)]
    pub(crate) fn inner(&self) -> &Mutex<BufPoolInner> {
        &self.inner
    }

    pub(crate) fn stat(&self) -> Arc<BufPoolStats> {
        self.stat.clone()
    }
}
