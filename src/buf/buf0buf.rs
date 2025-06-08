use crate::buf::buf0lru::BufLRU;
use crate::buf::buf0types::BufIoFix::BufIoRead;
use crate::buf::buf0types::{
    BufBlock, BufChunk, BufPage, BufPageState, BufPoolStats,
    FlushListAdapter, FreeListAdapter, LruAdapter, Request, BUF_GET_ALWAYS,
    BUF_GET_IF_IN_POOL, BUF_GET_NO_LATCH,
};
use crate::fil::fil0fil::Fil;
use crate::innodb::innodb0types::{
    DbErr, PageId, PageNo, SpaceId, UNIV_PAGE_SIZE,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::mtr::mtr0types::MtrState;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::ut_ad;
use crate::{
    lock_exclusive, lock_shared, some_or_panic, ut_2_power_up, ut_panic,
};
use intrusive_collections::{LinkedList, UnsafeRef};
use parking_lot::lock_api::RawRwLock;
use std::alloc::{alloc_zeroed, Layout};
use std::cell;
use std::cell::{Cell, RefCell};
use std::cmp::min;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};
use std::sync::{Arc, RwLock};
use tracing::info;

/// Buffer Pool manages the caching of database pages in memory.
///
/// The buffer pool is a critical component of the database system that caches
/// frequently accessed pages to reduce disk I/O operations. It implements
/// various page replacement algorithms and manages the lifecycle of pages
/// in memory.
pub struct BufPool {
    /// A sequence number used to count the number of buffer blocks removed from the end of the lru list.
    freed_page_clock: usize,
    /// Current size of the buffer pool in pages.
    curr_size: usize,
    /// Hash table mapping page IDs to buffer blocks
    page_hash: HashMap<PageId, Arc<RwLock<BufBlock>>>,
    /// List of modified pages that need to be flushed to disk
    flush_list: LinkedList<FlushListAdapter>,
    /// File system interface for I/O operations
    fil: Arc<RwLock<Fil>>,
    /// reference to BufLru.
    buf_lru: Arc<RefCell<BufLRU>>,
    /// List of memory chunks allocated for the buffer pool
    chunks: Vec<Arc<RwLock<BufChunk>>>,
    /// Statistics about the buffer pool
    stat: RefCell<BufPoolStats>,
}
impl BufPool {
    /// Creates a new Buffer Pool instance.
    ///
    /// # Parameters
    /// * `fil` - A reference to the file system interface for I/O operations
    ///
    /// # Returns
    /// A new BufPool instance with initialized data structures
    pub fn new(fil: Arc<RwLock<Fil>>, buf_lru: Arc<RefCell<BufLRU>>) -> Self {
        BufPool {
            freed_page_clock: 0,
            curr_size: 0,
            page_hash: Default::default(),
            flush_list: Default::default(),
            fil,
            buf_lru,
            chunks: vec![],
            stat: RefCell::new(BufPoolStats::new()),
        }
    }

    /// Increments the freed page counter.
    ///
    /// This method is called when a buffer block is removed from the LRU list
    /// and returned to the free list. It helps track how many pages have been
    /// freed during the buffer pool's lifetime.
    pub fn incr_freed_page_count(&mut self) {
        self.freed_page_clock += 1;
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
    pub fn get_free_block(&mut self) -> Option<Weak<RefCell<BufBlock>>> {
        if let Some(r) = self.buf_lru.borrow_mut().get_free_block() {
            info!("free block found");
            Some(Rc::downgrade(&r))
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
    pub fn get(&self, request: Request) -> Option<Arc<RwLock<BufBlock>>> {
        ut_ad!(request.mtr.get_mtr_state() == MtrState::MTR_ACTIVE);
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

        self.stat.borrow_mut().n_pages_get += 1;

        // TODO(Rahul): Understand the logic of `guess`.

        // TODO: Implement retry logic for getting a page for n number of times.

        // 1. First, check if the page is already in the buffer pool.
        let block_opt = self.page_hash.get(&request.page_id);

        match block_opt {
            // 2. The page is not in the buffer pool, and the request mode is BUF_GET_NO_LATCH.
            // Return None.
            None => {
                if request.mode == BUF_GET_IF_IN_POOL {
                    return None;
                }
            }
            Some(_) => {}
        }

        let block =
            some_or_panic!(block_opt, "failed to get block from page hash");
        {
            let block_ref = block.read().unwrap();
            // The page is being read.
            let page_read_io =
                block_ref.buf_block_get_page().borrow().buf_page_get_io_fix()
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
        block_ref.buf_block_get_page().borrow_mut().buf_page_io_fix_count();
        let buf_page = block_ref.buf_block_get_page();
        let access_time =
            self.buf_page_accessed_time(buf_page.borrow_mut().deref());

        self.set_accessed_make_young(block_ref, access_time);

        match request.rw_latch {
            ReadWriteLatch::SLatch => {
                lock_exclusive!(block_ref);
            }
            ReadWriteLatch::XLatch => {
                lock_shared!(block_ref);
            }
            ReadWriteLatch::NoLatch => {
                // The caller does not need any latch.
            }
        }

        if access_time == 0 {
            // TODO: Implement logic for read ahead.
        }

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
        min(ut_2_power_up!(self.curr_size / 32), 64)
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
        self.page_hash.get(&page_id).cloned()
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
    pub fn hash_remove_page(
        &mut self,
        space_id: SpaceId,
        page_no: PageNo,
    ) -> bool {
        let page_id = PageId { space_id, page_no };
        if let Some(block) = self.page_hash.remove(&page_id) {
            let block_ref = block.write().unwrap();
            let buf_page = block_ref.buf_block_get_page();
            buf_page.borrow_mut().set_is_in_hash_map(false);
            return true;
        }
        false
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
    pub fn peek(&self, space_id: SpaceId, page_no: PageNo) -> bool {
        let page_id = PageId { space_id, page_no };
        self.page_hash.contains_key(&page_id)
    }

    /// Handles the completion of an I/O operation on a buffer page.
    ///
    /// This method is called when an asynchronous I/O operation (read or write)
    /// on a buffer page completes. It updates the page state and performs
    /// any necessary post-processing.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page for which I/O has completed
    pub fn io_complete(&self, buf_page: BufPage) {}

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
        &mut self,
        space_id: SpaceId,
        page_no: PageNo,
        table_space_version: i64,
    ) -> Result<Option<Arc<RwLock<BufBlock>>>, DbErr> {
        /* 1. Check if the block is present in the buffer pool.*/
        let block = self.page_hash.get(&PageId { space_id, page_no });

        if let Some(buf_block) = block {
            return Ok(None);
        }

        /* 2. Check if the tablespace is being deleted or already deleted. */
        let fil = self.fil.read().unwrap();
        if fil
            .tablespace_deleted_or_being_deleted(space_id, table_space_version)
        {
            tracing::warn!(
                "Trying to read a page from a deleted tablespace or is being deleted: {}",
                space_id
            );
            return Ok(None);
        }

        /* 3. Allocate a new buffer block for the page. */
        let block = block.unwrap();
        let mut block_ref = block.write().unwrap();

        let buf_page = block_ref.buf_block_get_page();
        let mut buf_page_ref = buf_page.borrow_mut();
        self.buf_lru.borrow_mut().add_block(&mut buf_page_ref, false);

        self.page_init(space_id, page_no, block_ref.deref_mut());

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
    pub fn add_to_flush_list(&mut self, buf_page: &mut BufPage) {
        unsafe {
            self.flush_list.push_back(UnsafeRef::from_raw(buf_page));
        }
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
        buf_block: &mut BufBlock,
    ) {}

    /// Frees a buffer block and returns it to the free list.
    ///
    /// This method releases a buffer block that is no longer needed, making it
    /// available for reuse. It delegates to the LRU manager's block_free_non_file_page
    /// method to handle the actual freeing process.
    ///
    /// # Parameters
    /// * `buf_block` - The buffer block to free
    pub fn block_free(&mut self, buf_block: &mut BufBlock) {
        self.buf_lru.borrow_mut().block_free_non_file_page(buf_block);
    }

    /// Creates a new page in the buffer pool.
    ///
    /// This method allocates a buffer block for a new page with the specified
    /// space ID and page number. It first attempts to get a free block from the
    /// free list, then initializes it for the new page.
    ///
    /// # Parameters
    /// * `space_id` - The ID of the tablespace for the new page
    /// * `page_no` - The page number within the tablespace
    /// * `mtr` - The mini-transaction context for logging the operation
    ///
    /// # Returns
    /// * `Some(Rc<RefCell<BufBlock>>)` - A reference to the newly created buffer block
    /// * `None` - If no free blocks are available
    ///
    /// # Panics
    /// Panics in debug mode if the mini-transaction is not active
    pub fn create(
        &mut self,
        space_id: SpaceId,
        page_no: PageNo,
        mtr: &mut Mtr,
    ) -> Option<Rc<RefCell<BufBlock>>> {
        ut_ad!(mtr.get_mtr_state() == MtrState::MTR_ACTIVE);

        let free_block = self.get_free_block();
        if let Some(block) = free_block {
            if let Some(buf_block) = block.upgrade() {
                let mut block_ref = buf_block.borrow_mut();
                let block = block_ref.deref_mut();

                if block.buf_block_get_page().borrow().in_file() {
                    self.block_free(block);

                    // return using the Get methods.
                }

                self.page_init(space_id, page_no, block);

                return Some(buf_block.clone());
            }
        }

        None
    }

    /// Returns the number of blocks in the free list.
    ///
    /// This method delegates to the LRU manager to get the current size of the free list,
    /// which indicates how many buffer blocks are available for immediate reuse.
    ///
    /// # Returns
    /// * `usize` - The number of blocks in the free list
    pub fn get_free_list_size(&self) -> usize {
        self.buf_lru.borrow().get_free_list_size()
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
    /// # Panics
    /// Panics in debug mode if the memory size is not a multiple of the page size
    pub fn chunk_init(&mut self, mem_size: usize) -> Arc<RwLock<BufChunk>> {
        ut_ad!(mem_size % UNIV_PAGE_SIZE == 0);

        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        let mem_ptr = unsafe { alloc_zeroed(layout) };

        let no_of_pages = mem_size / UNIV_PAGE_SIZE;

        let mut buf_pages =
            Vec::<RefCell<Rc<RefCell<BufBlock>>>>::with_capacity(no_of_pages);

        ut_ad!(buf_pages.len() == 0);

        let mut buf_frame_ptr = mem_ptr;
        for _index in 0..no_of_pages {
            let buf_page =
                RefCell::new(Rc::new(RefCell::new(BufPage::new(0, 0))));
            let buf_block = RefCell::new(Rc::new(RefCell::new(
                BufBlock::new(buf_page.clone(), buf_frame_ptr),
            )));

            let buf_page_ref = buf_page.borrow();

            self.buf_lru
                .borrow_mut()
                .add_to_free_list(buf_page_ref.borrow_mut().deref_mut());

            let buf_block_weak = Rc::downgrade(&buf_block.borrow());

            buf_page.borrow().borrow_mut().set_buf_block_ref(buf_block_weak);

            buf_pages.push(buf_block);

            unsafe {
                buf_frame_ptr = buf_frame_ptr.add(UNIV_PAGE_SIZE);
            }
        }

        ut_ad!(buf_pages.len() == no_of_pages);

        let rt = Arc::new(RwLock::new(BufChunk {
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
    pub fn open(&mut self, pool_size: usize) {
        let chunk = self.chunk_init(pool_size);
        self.chunks.push(chunk);

        self.curr_size = pool_size;
    }

    /// Sets the first access time of the page and potentially makes it young in the LRU list.
    ///
    /// This method updates the access time of a buffer page and, if the page is considered
    /// old enough based on the access time, moves it to the start of the buffer pool's LRU list
    /// to prevent it from being evicted soon.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer block to update
    /// * `access_time` - The current access time to compare against the page's last access time
    pub fn set_accessed_make_young(
        &self,
        buf_page: &mut BufBlock,
        access_time: u64,
    ) {}

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
}
