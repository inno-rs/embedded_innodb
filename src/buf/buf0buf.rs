use crate::buf::buf0lru::BufLRU;
use crate::buf::buf0types::{
    BufBlock, BufChunk, BufPage, FlushListAdapter, FreeListAdapter,
    LruAdapter, Request, BUF_GET_ALWAYS, BUF_GET_IF_IN_POOL, BUF_GET_NO_LATCH,
};
use crate::fil::fil0fil::Fil;
use crate::fil::fil0types::SpaceId;
use crate::innodb::innodb0types::{DbErr, PageId, PageNo, UNIV_PAGE_SIZE};
use crate::mtr::mtr0mtr::Mtr;
use crate::mtr::mtr0types::MtrState;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use intrusive_collections::{LinkedList, UnsafeRef};
use log::warn;
use std::alloc::{alloc_zeroed, Layout};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::null_mut;
use std::rc::{Rc, Weak};
use std::sync::{Arc, RwLock};

/// Buffer Pool manages the caching of database pages in memory.
///
/// The buffer pool is a critical component of the database system that caches
/// frequently accessed pages to reduce disk I/O operations. It implements
/// various page replacement algorithms and manages the lifecycle of pages
/// in memory.
pub struct BufPool {
    /// Current size of the buffer pool in pages.
    curr_size: usize,
    /// Hash table mapping page IDs to buffer blocks
    page_hash: HashMap<PageId, RefCell<BufBlock>>,
    /// List of modified pages that need to be flushed to disk
    flush_list: LinkedList<FlushListAdapter>,
    /// List of free buffer blocks available for allocation
    free_list: LinkedList<FreeListAdapter>,
    /// List of buffer blocks ordered by least recently used algorithm
    lru_list: LinkedList<LruAdapter>,
    /// File system interface for I/O operations
    fil: Arc<RwLock<Fil>>,
    buf_lru: Weak<RefCell<BufLRU>>,
}
impl BufPool {
    /// Creates a new Buffer Pool instance.
    ///
    /// # Parameters
    /// * `fil` - A reference to the file system interface for I/O operations
    ///
    /// # Returns
    /// A new BufPool instance with initialized data structures
    pub fn new(fil: Arc<RwLock<Fil>>) -> Self {
        BufPool {
            curr_size: 0,
            page_hash: Default::default(),
            free_list: Default::default(),
            lru_list: Default::default(),
            flush_list: Default::default(),
            fil,
            buf_lru: Weak::new(),
        }
    }

    /// Sets the LRU manager for this buffer pool.
    pub fn set_buf_lru(&mut self, buf_lru: &Rc<RefCell<BufLRU>>) {
        self.buf_lru = Rc::downgrade(buf_lru);
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
        println!("free list size: {}", self.get_free_list_size());
        if let Some(buf_page) = self.free_list.pop_front() {
            let block = buf_page.buf_page_get_buf_block();
            Some(block)
        } else {
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
    pub fn get(&self, request: Request) -> Option<&BufBlock> {
        ut_ad!(request.mtr.get_mtr_state() == MtrState::MTR_ACTIVE);
        ut_ad!(
            request.rw_latch == ReadWriteLatch::RW_NO_LATCH
                || request.rw_latch == ReadWriteLatch::RW_S_LATCH
                || request.rw_latch == ReadWriteLatch::RW_X_LATCH
        );
        ut_ad!(
            request.mode != BUF_GET_NO_LATCH
                || request.rw_latch == ReadWriteLatch::RW_NO_LATCH
        );
        ut_ad!(
            request.mode == BUF_GET_ALWAYS
                || request.mode == BUF_GET_IF_IN_POOL
                || request.mode == BUF_GET_NO_LATCH
        );

        let block_opt =
            self.page_hash.get(&request.page_id).map(|block| block.borrow());

        match block_opt {
            None => {
                if request.mode == BUF_GET_IF_IN_POOL {
                    return None;
                }
            }
            Some(_) => {}
        }

        let block = block_opt.unwrap();

        match request.rw_latch {
            ReadWriteLatch::RW_S_LATCH => {}
            ReadWriteLatch::RW_X_LATCH => {}
            ReadWriteLatch::RW_NO_LATCH => {
                // The caller does not need any latch.
            }
        }

        None
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
    ) -> Option<&BufBlock> {
        let page_id = PageId { space_id, page_no };
        // self.page_hash.get(&page_id).map(|block| block.borrow())
        None
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
    /// This method prepares for reading a page from disk into the buffer pool.
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
    ) -> Result<Option<RefCell<BufBlock>>, DbErr> {
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
            warn!(
                "Trying to read a page from a deleted tablespace or is being deleted: {}",
                space_id
            );
            return Ok(None);
        }

        /* 3. Allocate a new buffer block for the page. */
        let block = block.unwrap();
        self.buf_lru.upgrade().unwrap().borrow_mut().add_block(
            block.borrow_mut().buf_block_get_page().borrow().deref(),
        );

        self.page_init(space_id, page_no, block.clone().get_mut());

        let ret = Some(block.clone());
        Ok(ret)
    }

    pub fn add_to_free_list(&mut self, buf_page: &mut BufPage) {
        unsafe {
            self.free_list.push_back(UnsafeRef::from_raw(buf_page));
        }
        buf_page.set_is_in_lru_list(true);
    }

    pub fn add_to_flush_list(&mut self, buf_page: &mut BufPage) {
        unsafe {
            self.flush_list.push_back(UnsafeRef::from_raw(buf_page));
        }
        buf_page.set_is_in_flush_list(true);
    }

    pub fn add_to_lru_list(&mut self, buf_page: &mut BufPage) {
        unsafe {
            self.lru_list.push_back(UnsafeRef::from_raw(buf_page));
        }
        buf_page.set_is_in_lru_list(true);
    }

    /// Initializes a page in the buffer pool.
    pub fn page_init(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
        buf_block: &mut BufBlock,
    ) {
    }

    pub fn block_free(&mut self, buf_block: &mut BufBlock) {
        self.buf_lru
            .upgrade()
            .unwrap()
            .borrow_mut()
            .block_free_non_file_page(buf_block);
    }

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

    pub fn get_free_list_size(&self) -> usize {
        self.free_list.iter().count()
    }

    pub fn chunk_init(&mut self, mem_size: usize) -> Box<BufChunk> {
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

            self.add_to_free_list(buf_page_ref.borrow_mut().deref_mut());

            let buf_block_weak = Rc::downgrade(&buf_block.borrow());

            buf_page.borrow().borrow_mut().set_buf_block_ref(buf_block_weak);

            buf_pages.push(buf_block);

            unsafe {
                buf_frame_ptr = buf_frame_ptr.add(UNIV_PAGE_SIZE);
            }
        }

        ut_ad!(buf_pages.len() == no_of_pages);

        let rt = Box::new(BufChunk {
            mem_size,
            size: no_of_pages,
            mem: mem_ptr,
            buf_pages,
        });

        rt
    }
}
