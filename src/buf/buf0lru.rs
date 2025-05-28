use crate::buf::buf0buf::BufPool;
use crate::buf::buf0types::BufPageState::{
    BufBlockNotUsed, BufBlockReadyForUse,
};
use crate::buf::buf0types::{BufBlock, BufPage, BufPageState, LruAdapter};
use crate::ut::ut0dbg::{ut_a, ut_ad};
use intrusive_collections::{LinkedList, UnsafeRef};
use log::info;
use std::cell::RefCell;
use std::rc::Weak;
use std::sync::{Arc, RwLock};

/// Manages the Least Recently Used (LRU) list of buffer pages.
///
/// The BufLRU component is responsible for implementing the LRU algorithm
/// for the buffer pool. It manages the movement of pages between the free list
/// and the LRU list, and handles the eviction of pages when the buffer pool
/// is full.
pub struct BufLRU {
    /// Reference to the buffer pool that this LRU manager operates on
    buf_pool: Arc<RwLock<BufPool>>,
    lru_list: RefCell<Weak<RefCell<LinkedList<LruAdapter>>>>,
}

impl BufLRU {
    /// Creates a new LRU manager for the given buffer pool.
    ///
    /// # Parameters
    /// * `buf_pool` - A reference to the buffer pool to manage
    ///
    /// # Returns
    /// A new BufLRU instance
    pub fn new(buf_pool: Arc<RwLock<BufPool>>) -> Self {
        BufLRU { buf_pool, lru_list: RefCell::new(Default::default()) }
    }

    pub fn add_block(&mut self, buf_page: &BufPage) {
        self.add_block_low(buf_page);
    }

    fn add_block_low(&mut self, buf_page: &BufPage) {
        let page = unsafe { UnsafeRef::from_raw(buf_page) };
        let lru_list = self.lru_list.borrow_mut();
        if let Some(lru) = lru_list.upgrade() {
            let mut lru_ref = lru.borrow_mut();
            lru_ref.push_back(page);
        } else {
            ut_a!(false, "LRU list is not initialized");
        }
    }

    /// Gets a free block from the buffer pool's free list without modifying the LRU list.
    ///
    /// This method attempts to retrieve a block directly from the free list
    /// without moving blocks from the LRU list if the free list is empty.
    ///
    /// # Returns
    /// * `Some(BufBlock)` - A free buffer block if available on the free list
    /// * `None` - If the free list is empty
    pub fn get_free_only(&mut self) -> Option<BufBlock> {
        let pool = self.buf_pool.write().unwrap();

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
    pub fn get_free_block(&mut self) -> Option<BufBlock> {
        // First try to get a block from the free list
        if let Some(block) = self.get_free_only() {
            return Some(block);
        }

        // If free list is empty, we should move blocks from LRU list
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

        ut_ad!(block.buf_block_get_page().is_in_free_list() == false);
        ut_ad!(block.buf_block_get_page().is_in_flush_list() == false);
        ut_ad!(block.buf_block_get_page().is_in_lru_list() == false);

        block.buf_block_set_state(BufBlockNotUsed);

        let mut pool = self.buf_pool.write().unwrap();
        pool.add_to_free_list(block.buf_block_get_page_mut());

        ut_a!(block.buf_block_get_page_mut().is_in_lru_list())
    }
}
