use crate::buf::buf0buf::BufPool;
use crate::buf::buf0types::BufPageState::{
    BufBlockNotUsed, BufBlockReadyForUse,
};
use crate::buf::buf0types::{
    BufBlock, BufPage, BufPageState, FreeListAdapter, LruAdapter,
};
use crate::ut::ut0dbg::{ut_a, ut_ad};
use intrusive_collections::{LinkedList, UnsafeRef};
use log::info;
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};
use std::sync::{Arc, RwLock};

/// Manages the Least Recently Used (LRU) list of buffer pages.
///
/// The BufLRU component is responsible for implementing the LRU algorithm
/// for the buffer pool. It manages the movement of pages between the free list
/// and the LRU list, and handles the eviction of pages when the buffer pool
/// is full.
pub struct BufLRU {
    /// Reference to the buffer pool that this LRU manager operates on
    buf_pool: Weak<RefCell<BufPool>>,
    /// List of buffer blocks ordered by least recently used algorithm
    lru_list: Arc<RefCell<LinkedList<LruAdapter>>>,
    /// List of free buffer blocks available for allocation
    free_list: Arc<RefCell<LinkedList<FreeListAdapter>>>,
}

impl BufLRU {
    /// Creates a new LRU manager for the given buffer pool.
    ///
    /// # Parameters
    /// * `buf_pool` - A reference to the buffer pool to manage
    ///
    /// # Returns
    /// A new BufLRU instance
    pub fn new() -> Self {
        BufLRU {
            buf_pool: Default::default(),
            lru_list: Default::default(),
            free_list: Default::default(),
        }
    }

    pub fn init(&mut self, buf_pool: Weak<RefCell<BufPool>>) {
        let buf_pool_ref = buf_pool.upgrade();
        if let Some(buf_pool) = buf_pool_ref {
            self.buf_pool = Rc::downgrade(&buf_pool);
        }
    }

    pub fn add_block(&mut self, buf_page: &BufPage) {
        self.add_block_low(buf_page);
    }

    /// Moves a page to the start of the buffer pool LRU list.
    /// This high level function can be used
    /// to prevent an important page from slipping out of the buffer pool.
    pub fn make_young(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.in_file());

        self.remove_block(buf_page);
        self.add_block_low(buf_page);
    }

    fn remove_block(&mut self, buf_page: &mut BufPage) {
        let mut lru_ref = self.lru_list.borrow_mut();
        unsafe {
            let mut cursor = lru_ref.cursor_mut_from_ptr(buf_page);
            cursor.remove();
        }
        buf_page.set_is_in_lru_list(false);
    }

    fn add_block_low(&mut self, buf_page: &BufPage) {
        let page = unsafe { UnsafeRef::from_raw(buf_page) };
        let mut lru_list = self.lru_list.borrow_mut();
        lru_list.push_back(page);
    }

    /// Gets a free block from the buffer pool's free list without modifying the LRU list.
    ///
    /// This method attempts to retrieve a block directly from the free list
    /// without moving blocks from the LRU list if the free list is empty.
    ///
    /// # Returns
    /// * `Some(BufBlock)` - A free buffer block if available on the free list
    /// * `None` - If the free list is empty
    pub fn get_free_only(&mut self) -> Option<Rc<RefCell<BufBlock>>> {
        let buf_page = self.free_list.borrow_mut().pop_front();

        if let Some(buf_page) = buf_page {
            let buf_block = buf_page.buf_page_get_buf_block();
            return buf_block.upgrade();
        }

        None
    }

    /// Gets a free block from the buffer pool, potentially moving blocks from the LRU list.
    ///
    /// This method first tries to get a block from the free list. If the free list
    /// is empty, it moves blocks from the end of the LRU list (least recently used)
    /// to the free list, and then returns one of those blocks.
    ///
    /// # Returns
    /// * `Some(BufBlock)` - A free buffer block
    /// * `None` - If no blocks could be freed (should not happen in a properly sized buffer pool)
    pub fn get_free_block(&mut self) -> Option<Rc<RefCell<BufBlock>>> {
        // First try to get a block from the free list
        if let Some(block) = self.get_free_only() {
            return Some(block);
        }

        // If the free list is empty, we should move blocks from LRU list
        // This is a simplified implementation that just returns None for now
        None
    }

    /// Frees a non-file page block and returns it to the free list.
    ///
    /// This method handles the freeing of buffer blocks that are not associated
    /// with file pages (e.g., blocks used for internal data structures).
    ///
    /// # Parameters
    /// * `block` - The buffer block to be freed
    ///
    /// # Panics
    /// Panics if the block is not in a valid state to be freed
    pub fn block_free_non_file_page(&mut self, block: &mut BufBlock) {
        match block.get_state() {
            n if n == BufPageState::BufBlockMemory
                || n == BufBlockReadyForUse =>
            {
                info!("bufblock state: {:?}", n);
            }
            n => {
                ut_a!(
                    false,
                    "Block is not in a valid state to be freed: {:?}",
                    n
                );
            }
        }

        let buf_page = block.buf_block_get_page();
        let page_ref = buf_page.borrow();
        ut_ad!(page_ref.is_in_free_list() == false);
        ut_ad!(page_ref.is_in_flush_list() == false);
        ut_ad!(page_ref.is_in_lru_list() == false);

        block.buf_block_set_state(BufBlockNotUsed);

        let page = block.buf_block_get_page();

        self.add_to_free_list(page.borrow_mut().deref_mut());

        ut_a!(page.borrow().is_in_lru_list())
    }

    pub fn add_to_free_list(&mut self, buf_page: &mut BufPage) {
        let mut free_list_ref = self.free_list.borrow_mut();
        unsafe {
            free_list_ref.push_back(UnsafeRef::from_raw(buf_page));
        }
        buf_page.set_is_in_lru_list(true);
    }

    pub fn buf_page_remove_from_lru_list(&mut self, buf_page: &mut BufPage) {
        self.remove_block(buf_page);
    }

    pub fn get_free_list_size(&self) -> usize {
        self.free_list.borrow().iter().count()
    }
}
