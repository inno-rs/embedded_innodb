use std::cell::RefCell;
use std::cmp::min;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};
use std::sync::{Arc, RwLock};

use intrusive_collections::{LinkedList, UnsafeRef};
use tracing::info;

use crate::buf::buf0buf::BufPool;
use crate::buf::buf0types::BufPageState::{
    BufBlockNotUsed, BufBlockReadyForUse,
};
use crate::buf::buf0types::{
    BufBlock, BufIoFix, BufPage, BufPageState, FreeListAdapter, LruAdapter,
    Request,
};
use crate::innodb::innodb0types::PageId;
use crate::mtr::mtr0mtr::Mtr;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use crate::{some_or_panic, ut_panic};

/// The denominator of lru_old_ratio.
pub const OLD_RATIO_DIV: usize = 1024;
/// Maximum value of lru_old_ratio.
pub const OLD_RATIO_MAX: usize = OLD_RATIO_DIV;
/// Minimum value of lru old ratio.
pub const OLD_RATIO_MIN: usize = 51;

/// Minimum Lru list length for which lru_old_pointer is defined. 8MB of 16k pages.
pub const OLD_MIN_LENGTH: usize = 512;

/// The number of blocks from the BufLru::lru_old_head onward, including the block pointed to,
/// must be of BufLru::old_ratio/OLD_RATIO_DIV of the whole lru list length,
/// except the tolerance defined below is allowed.
const OLD_TOLERANCE: usize = 20;
/// Minimum amount of non-old blocks when the BufLru::lru_old_head exists.
const NON_MIN_LENGTH: usize = 5;

/// Represents the status of a buffer block after attempting to free it.
///
/// This enum is used to indicate the result of an attempt to free a buffer block,
/// providing information about why a block could not be freed if applicable.
pub enum BlockStatus {
    /// The block was successfully freed and can be reused.
    Freed = 0,
    /// The block cannot be relocated because it is either I/O fixed or buffer fixed.
    CannotRelocate,
    /// The block was not freed for some other reason, such as being modified.
    NotFreed,
}

/// Manages the Least Recently Used (LRU) list of buffer pages.
///
/// The BufLRU component is responsible for implementing the LRU algorithm
/// for the buffer pool. It manages the movement of pages between the free list
/// and the LRU list, and handles the eviction of pages when the buffer pool
/// is full.
pub struct BufLRU {
    /// Reference to the buffer pool that this LRU manager operates on
    buf_pool: Weak<RefCell<BufPool>>,
    /// Reserve old_ratio/OLD_RATIO_DIV for old blocks.
    old_ratio: usize,
    /// List of buffer blocks ordered by least recently used algorithm
    lru_list: Arc<RefCell<LinkedList<LruAdapter>>>,
    /// Pointer to old_ratio/buf_lru_ratio_div. None if LRU length less than BUF_LRU_MIN_LEN.
    lru_old_head: Option<UnsafeRef<BufPage>>,
    /// Length of the Lru list from the node pointed to by the lru_old_head to the end. This section consists of old blocks.
    lru_old_len: usize,
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
            old_ratio: 0,
            lru_list: Default::default(),
            lru_old_head: None,
            lru_old_len: 0,
            free_list: Default::default(),
        }
    }

    /// Initializes the LRU manager with a reference to the buffer pool.
    ///
    /// This method sets up the connection between the LRU manager and the buffer pool,
    /// allowing the LRU manager to access and manage the buffer pool's pages.
    ///
    /// # Parameters
    /// * `buf_pool` - A weak reference to the buffer pool to manage
    pub fn init(&mut self, buf_pool: Weak<RefCell<BufPool>>) {
        let buf_pool_ref = buf_pool.upgrade();
        if let Some(buf_pool) = buf_pool_ref {
            self.buf_pool = Rc::downgrade(&buf_pool);
        }
    }

    /// Tries to remove LRU flushed blocks from the end of the LRU list and put them to the free list.
    ///
    /// This method improves the efficiency of insert operations by removing flushed pages
    /// from non-unique non-clustered indexes from the buffer pool. This allows their inserts
    /// to be redirected to the insert buffer. Without this optimization, the flushed blocks
    /// might be modified again before read operations need new buffer blocks, wasting the I/O
    /// work done in flushing.
    ///
    /// # Returns
    /// * `bool` - True if a block was successfully freed, false otherwise
    pub fn try_free_flushed_block(&mut self) {}

    /// Adds a buffer page to the LRU list.
    ///
    /// This is a high-level method that adds a buffer page to the LRU list
    /// by delegating to the internal `add_block_low` method.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the LRU list
    pub fn add_block(&mut self, buf_page: &mut BufPage, old: bool) {
        self.add_block_low(buf_page, old);
    }

    /// Moves a page to the start of the buffer pool LRU list.
    ///
    /// This high-level function can be used to prevent an important page from being
    /// evicted from the buffer pool. By moving the page to the start of the LRU list,
    /// it becomes the most recently used page and will be one of the last candidates
    /// for eviction when the buffer pool needs to free space.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to make young
    ///
    /// # Panics
    /// Panics if the page is not in a file
    pub fn make_young(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.in_file());

        self.remove_block(buf_page);
        self.add_block_low(buf_page, false);
    }

    /// Moves a page to the end of the buffer pool LRU list.
    ///
    /// This method makes a page the least recently used by moving it to the end
    /// of the LRU list. This is useful when a page is known to be less important
    /// or unlikely to be accessed again soon, making it a prime candidate for
    /// eviction when the buffer pool needs to free space.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to move to the end of the LRU list
    ///
    /// # Panics
    /// Panics if the page is not in a file
    pub fn make_block(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.in_file());

        self.remove_block(buf_page);
        self.add_block_to_end_low(buf_page);
    }

    /// Removes a buffer page from the LRU list.
    ///
    /// This internal method removes a buffer page from the LRU list and updates
    /// the page's status to indicate it is no longer in the LRU list.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to remove from the LRU list
    fn remove_block(&mut self, buf_page: &mut BufPage) {
        let mut lru_ref = self.lru_list.borrow_mut();
        unsafe {
            let mut cursor = lru_ref.cursor_mut_from_ptr(buf_page);
            cursor.remove();
        }
        buf_page.set_is_in_lru_list(false);
    }

    /// Adds a buffer page to the end of the LRU list.
    ///
    /// This method is intended to add a buffer page to the end of the LRU list,
    /// making it the least recently used page and a candidate for early eviction.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the end of the LRU list
    ///
    /// # Note
    /// This method is currently a stub and needs to be implemented.
    fn add_block_to_end_low(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.in_file());
        ut_ad!(!buf_page.is_in_lru_list());
        // TODO: Implement this method to add the page to the end of the LRU list
    }

    /// Adds a buffer page to the LRU list.
    ///
    /// This is a low-level internal method that performs the actual addition
    /// of a buffer page to the LRU list. Depending on the `old` parameter and the
    /// current state of the LRU list, it either:
    /// - Adds the page to the front of the list (for new pages)
    /// - Adds the page after the old pointer (for old pages)
    ///
    /// The method also updates the page's 'old' status accordingly.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the LRU list
    /// * `old` - Whether to add the page to the old section of the LRU list
    fn add_block_low(&mut self, buf_page: &mut BufPage, old: bool) {
        let length = self.lru_list.borrow().iter().count();
        if old == false || length < OLD_MIN_LENGTH {
            let page = unsafe { UnsafeRef::from_raw(buf_page) };
            let mut lru_list = self.lru_list.borrow_mut();
            lru_list.push_front(page);
        } else {
            let mut lru_list_ref = self.lru_list.borrow_mut();
            unsafe {
                let mut cursor = lru_list_ref.cursor_mut_from_ptr(buf_page);
                cursor.insert_after(UnsafeRef::from_raw(buf_page));
                buf_page.set_is_in_lru_list(false);
            }
        }

        let buf_pool = self.buf_pool.upgrade();
        if let Some(buf_pool) = buf_pool {
            if length > OLD_MIN_LENGTH {
                ut_ad!(self.lru_old_head.is_some());

                self.buf_page_set(buf_page, old);
            } else if length == OLD_MIN_LENGTH {
            } else {
                self.buf_page_set(buf_page, self.lru_old_head.is_some());
            }
        }
    }

    /// Sets the 'old' status of a buffer page.
    ///
    /// This method marks a buffer page as either 'old' or 'new' in the LRU list.
    /// Pages marked as 'old' are candidates for eviction before 'new' pages.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to update
    /// * `old` - Whether to mark the page as old (true) or new (false)
    ///
    /// # Panics
    /// Panics if the page is not in a file or not in the LRU list
    fn buf_page_set(&mut self, buf_page: &mut BufPage, old: bool) {
        ut_a!(buf_page.in_file());
        ut_ad!(buf_page.is_in_lru_list());

        buf_page.set_old(old);
    }

    /// Adjusts the length of the old section in the LRU list.
    ///
    /// This method moves the lru_old_head pointer so that the total number of items
    /// marked as old are within the limits defined by the old_ratio. It ensures that
    /// the old section of the LRU list maintains the proper proportion of the total list.
    ///
    /// The method handles both increasing and decreasing the old section size as needed.
    fn old_adjust_len(&mut self) {
        struct PageStateChange {
            page_id: PageId,
            old: bool,
        }
        ut_ad!(self.old_ratio <= OLD_RATIO_MAX);
        ut_ad!(self.old_ratio >= OLD_RATIO_MIN);

        let mut changes: Vec<PageStateChange> = vec![];
        {
            let lru_length = self.lru_list.borrow().iter().count();
            let new_length = min(
                lru_length * self.old_ratio / OLD_RATIO_DIV,
                lru_length - OLD_TOLERANCE - NON_MIN_LENGTH,
            );

            let mut lru_old_length = self.lru_old_len;
            let buf_page = self.lru_old_head.take().unwrap();
            let lru_list = self.lru_list.borrow_mut();

            // move the pointer pointing to lru_old_head to increase the old section
            while new_length > lru_old_length + OLD_TOLERANCE {
                let mut cursor =
                    unsafe { lru_list.cursor_from_ptr(buf_page.deref()) };
                cursor.move_prev();

                let new_head = cursor.get().unwrap();
                let head_ref = unsafe { UnsafeRef::from_raw(new_head) };
                self.lru_old_head = Some(head_ref.clone());
                self.lru_old_len += 1;

                lru_old_length = self.lru_old_len;

                changes.push(PageStateChange {
                    page_id: new_head.get_page_id(),
                    old: true,
                });
            }

            while lru_old_length > new_length + OLD_TOLERANCE {
                let mut cursor =
                    unsafe { lru_list.cursor_from_ptr(buf_page.deref()) };

                cursor.move_next();

                let new_head = cursor.get().unwrap();
                let head_ref = unsafe { UnsafeRef::from_raw(new_head) };
                self.lru_old_head = Some(head_ref.clone());
                self.lru_old_len -= 1;
                lru_old_length = self.lru_old_len;

                changes.push(PageStateChange {
                    page_id: new_head.get_page_id(),
                    old: true,
                });
            }
        }

        let mut mtr = Mtr::new();
        let buf_pool =
            some_or_panic!(self.buf_pool.upgrade(), "buf_pool is empty");
        let mut buf_pool_ref = buf_pool.borrow_mut();
        changes.iter().for_each(|change| {
            buf_pool_ref.get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: change.page_id,
                mode: 0,
                mtr: &mut mtr,
            });
        });
    }

    /// Updates the old_ratio field based on the provided percentage.
    ///
    /// This method calculates a new ratio based on the provided percentage and
    /// optionally adjusts the LRU list's old section length accordingly.
    ///
    /// # Parameters
    /// * `old_pct` - The percentage of the buffer pool to reserve for old pages
    /// * `adjust` - Whether to adjust the LRU list's old section length immediately
    pub fn old_ratio_update(&mut self, old_pct: usize, adjust: bool) {
        let mut ratio = (old_pct * OLD_RATIO_DIV) / 100;

        ratio =
            if ratio < OLD_RATIO_MIN { OLD_RATIO_MIN } else { OLD_RATIO_MAX };

        if adjust {
            if ratio != self.old_ratio {
                self.old_ratio = ratio;

                self.old_adjust_len();
            }
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
    pub fn get_free_only(&mut self) -> Option<Arc<RwLock<BufBlock>>> {
        let buf_page = self.free_list.borrow_mut().pop_front();

        if let Some(buf_page) = buf_page {
            let buf_block = buf_page.buf_page_get_buf_block();
            if let Some(block) = buf_block.upgrade().clone() {
                let mut block_ref = block.write().unwrap();

                {
                    let buf_page = block_ref.buf_block_get_page();
                    let mut buf_page_ref = buf_page.borrow_mut();

                    ut_ad!(buf_page_ref.is_in_free_list());
                    ut_ad!(buf_page_ref.is_in_lru_list() == false);
                    ut_ad!(buf_page_ref.is_in_flush_list() == false);
                    ut_ad!(buf_page_ref.in_file() == false);

                    // Set the page is not in free list anymore
                    buf_page_ref.set_is_in_free_list(false);
                }

                block_ref.buf_block_set_state(BufBlockReadyForUse);

                return Some(block.clone());
            }

            ut_panic!("cannot set state for buf_block");
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
    pub fn get_free_block(&mut self) -> Option<Arc<RwLock<BufBlock>>> {
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
        match block.buf_block_get_state() {
            n if n == BufPageState::BufBlockMemory
                || n == BufBlockReadyForUse =>
            {
                info!("bufblock state: {:?}", n);
            }
            n => {
                ut_panic!(
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

    /// Adds a buffer page to the free list.
    ///
    /// This method adds a buffer page to the free list, making it available for
    /// reuse. It also updates the page's status to indicate it is now in the LRU list.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to add to the free list
    pub fn add_to_free_list(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.is_in_free_list() == false);
        let mut free_list_ref = self.free_list.borrow_mut();
        unsafe {
            free_list_ref.push_back(UnsafeRef::from_raw(buf_page));
        }
        buf_page.set_is_in_free_list(true);
    }

    /// Removes a buffer page from the LRU list.
    ///
    /// This is a public wrapper around the internal `remove_block` method,
    /// providing external access to remove a page from the LRU list.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to remove from the LRU list
    pub fn buf_page_remove_from_lru_list(&mut self, buf_page: &mut BufPage) {
        self.remove_block(buf_page);
    }

    /// Returns the number of blocks in the free list.
    ///
    /// This method counts and returns the current number of buffer blocks
    /// available in the free list.
    ///
    /// # Returns
    /// * `usize` - The number of blocks in the free list
    pub fn get_free_list_size(&self) -> usize {
        self.free_list.borrow().iter().count()
    }

    /// Attempts to free a buffer block from the LRU list.
    ///
    /// This method checks if a buffer page can be freed based on its current state.
    /// It verifies that the page is in a file and in the LRU list, and checks
    /// whether the page can be relocated and if it has been modified.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to attempt to free
    ///
    /// # Returns
    /// * `BlockStatus` - The status indicating whether the block was freed and if not, why
    pub fn free_block(&mut self, buf_page: &mut BufPage) -> BlockStatus {
        ut_ad!(buf_page.in_file());
        ut_ad!(buf_page.is_in_lru_list());
        // If page in flush_list, then it needs to be modified.
        // If not modified, then it should not be present in the flush list.
        ut_ad!(
            (buf_page.is_in_flush_list() && buf_page.buf_page_modified())
                || (!buf_page.is_in_flush_list()
                    && !buf_page.buf_page_modified())
        );

        if buf_page.buf_page_can_relocate() == false {
            return BlockStatus::CannotRelocate;
        }

        if buf_page.buf_page_modified() {
            return BlockStatus::NotFreed;
        }

        BlockStatus::Freed
    }

    /// Removes a buffer block from the LRU list and the buffer pool hash table.
    ///
    /// This method performs a complete removal of a buffer page from the buffer pool's
    /// data structures. It checks that the page is in a valid state for removal,
    /// removes it from the LRU list, updates the freed page counter, and removes
    /// the page from the buffer pool's hash table.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page to remove
    ///
    /// # Returns
    /// * `BufPageState` - The new state of the buffer page after removal (typically BufBlockMemory)
    ///
    /// # Panics
    /// Panics if the page has I/O operations in progress, if the buffer pool reference is invalid,
    /// or if the page is in an invalid state for removal.
    pub fn block_remove_hashed_page(
        &mut self,
        buf_page: &mut BufPage,
    ) -> BufPageState {
        ut_a!(buf_page.buf_page_get_io_fix() == BufIoFix::BufIoNone);
        ut_a!(buf_page.buf_page_io_fix_count() == 0);

        self.remove_block(buf_page);

        if let Some(buf_pool) = self.buf_pool.upgrade() {
            buf_pool.borrow_mut().incr_freed_page_count();
        } else {
            ut_panic!("buf_pool reference not found");
        }

        match buf_page.get_state() {
            BufPageState::BufBlockFilePage => {
                if let Some(buf_block) =
                    buf_page.buf_page_get_buf_block().upgrade()
                {
                    let mut buf_block_ref = buf_block.write().unwrap();
                    buf_block_ref.buf_block_incr_clock();
                } else {
                    ut_panic!("buf_block cannot be accessed from buf_page");
                }
            }
            _rest_of_states => {
                ut_panic!(
                    "buf_page should not be in state: {}",
                    _rest_of_states
                );
            }
        }

        let space_id = buf_page.get_space_id();
        let page_no = buf_page.get_page_no();

        // Get the page from the buffer pool hashmap.
        let buf_pool = self.buf_pool.upgrade().unwrap();
        buf_pool.borrow_mut().hash_remove_page(space_id, page_no);

        match buf_page.get_state() {
            BufPageState::BufBlockFilePage => {}
            _rest_of_states => {
                ut_panic!(
                    "invalid state for removal, space_id: {}, page_no: {}",
                    space_id,
                    page_no
                );
            }
        }

        BufPageState::BufBlockMemory
    }
}
