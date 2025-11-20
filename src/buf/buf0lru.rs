use std::{
    cmp::min,
    ops::Deref,
    sync::{Arc, RwLock, RwLockWriteGuard, Weak, atomic::Ordering::Relaxed},
};

use intrusive_collections::{LinkedList, UnsafeRef};

use crate::{
    buf::{
        buf0buf::BufPoolInner,
        buf0types::{
            BufBlock, BufIoFix, BufPage, BufPageState,
            BufPageState::{BufBlockNotUsed, BufBlockReadyForUse},
            BufPoolStats, FreeListAdapter, LruAdapter,
        },
    },
    innodb::innodb0types::PageId,
    ut::ut0dbg::{ut_a, ut_ad},
    ut_2_power_up, ut_panic,
};

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

/// How many pages deep into the LRU list we scan looking for
/// candidates to “top up” our free‐list.
const FREE_SEARCH_LEN: usize = 100;



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
/// and the LRU list and handles the eviction of pages when the buffer pool
/// is full.
pub struct BufLRU {
    /// Reference to the buffer pool that this LRU manager operates on
    //buf_pool: Weak<Mutex<BufPool>>,
    /// Reserve old_ratio/OLD_RATIO_DIV for old blocks.
    old_ratio: usize,
    /// List of buffer blocks ordered by least recently used algorithm
    lru_list: LinkedList<LruAdapter>,
    /// Pointer to old_ratio/buf_lru_ratio_div. None if LRU length less than BUF_LRU_MIN_LEN.
    lru_old_head: Option<UnsafeRef<BufPage>>,
    /// Length of the Lru list from the node pointed to by the lru_old_head to the end. This section consists of old blocks.
    lru_old_len: usize,
    /// List of free buffer blocks available for allocation
    free_list: LinkedList<FreeListAdapter>,
    /// Tracking the size of the free list
    free_list_size: usize,
    /// Tracking the lru size
    lru_size: usize,
    ///It’s the minimum time (in milliseconds) that a page in the old sublist must stay untouched before InnoDB allows it to be promoted (“made young”) when accessed.
    old_threshold_ms: u64
}

impl BufLRU {
    /// Creates a new LRU manager for the given buffer pool.
    ///
    /// # Arguments
    ///
    /// * `buf_pool` - A reference to the buffer pool to manage
    ///
    /// # Returns
    ///
    /// A new BufLRU instance
    ///
    pub fn new() -> Self {
        BufLRU {
            old_ratio: 37,  // default = 3.6% of LRU
            lru_list: Default::default(),
            lru_old_head: None,
            lru_old_len: 0,
            free_list: Default::default(),
            free_list_size: 0,
            lru_size: 0,
            old_threshold_ms:1000,
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
    ///
    /// `bool` - True if a block was successfully freed, false otherwise
    ///
    pub fn try_free_flushed_block(&mut self) {}

    /// Moves a page to the start of the buffer pool LRU list.
    ///
    /// This high-level function can be used to prevent an important page from being
    /// evicted from the buffer pool. By moving the page to the start of the LRU list,
    /// it becomes the most recently used page and will be one of the last candidates
    /// for eviction when the buffer pool needs to free space.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to make young
    ///
    /// # Panics
    ///
    /// Panics if the page is not in a file
    ///
    pub fn make_young(&mut self, buf_page: &mut BufPage, freed_page_clock:usize) {
        ut_ad!(buf_page.in_file());

        self.remove_block(buf_page);
        self.add_block_low(buf_page, false, freed_page_clock);
    }

    /// Moves a page to the end of the buffer pool LRU list.
    ///
    /// This method makes a page the least recently used by moving it to the end
    /// of the LRU list. This is useful when a page is known to be less important
    /// or unlikely to be accessed again soon, making it a prime candidate for
    /// eviction when the buffer pool needs to free space.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to move to the end of the LRU list
    ///
    /// # Panics
    ///
    /// Panics if the page is not in a file
    ///
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
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to remove from the LRU list
    ///
    fn remove_block(&mut self, buf_page: &mut BufPage) {
        let lru_ref = &mut self.lru_list;
        unsafe {
            let mut cursor = lru_ref.cursor_mut_from_ptr(buf_page);
            cursor.remove();
        }
        self.mark_page_not_in_lru_list(buf_page);
    }

    /// Adds a buffer page to the end of the LRU list.
    ///
    /// This method is intended to add a buffer page to the end of the LRU list,
    /// making it the least recently used page and a candidate for early eviction.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to add to the end of the LRU list
    ///
    pub fn add_block_to_end_low(&mut self, buf_page: &mut BufPage) {
        ut_ad!(buf_page.in_file());
        ut_ad!(!buf_page.is_in_lru_list());

        // 1) Append at the back (least recently used)
        let page_ref = unsafe { UnsafeRef::from_raw(buf_page) };
        self.lru_list.push_back(page_ref);

        // 2) Mark it as in the list
        self.mark_page_in_lru_list(buf_page);

        // 3) Update “old” marking based on list size
        let length = self.lru_size;
        if length > OLD_MIN_LENGTH {
            // Once we exceed the threshold, everything beyond is “old”
            ut_ad!(self.lru_old_head.is_some());
            self.buf_page_set(buf_page, /* old = */ true);
        } else if length < OLD_MIN_LENGTH {
            // Still building up: use whichever region we’ve already marked
            self.buf_page_set(buf_page, self.lru_old_head.is_some());
        }
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
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to add to the LRU list
    /// * `old` - Whether to add the page to the old section of the LRU list
    ///
    pub(crate) fn add_block_low(&mut self, buf_page: &mut BufPage, old: bool, freed_page_clock:usize) {
        let length = self.lru_size;
        if !old || length < OLD_MIN_LENGTH {
            // front‐insert
            let page = unsafe { UnsafeRef::from_raw(buf_page) };
            self.lru_list.push_front(page);
            buf_page.set_freed_page_clock(freed_page_clock);
        } else {
            let old_head = self
                .lru_old_head
                .as_ref()
                .expect("old-head must exist once length >= OLD_MIN_LENGTH");

            // position at old-head and insert after it
            let mut cursor = unsafe {
                self.lru_list
                    .cursor_mut_from_ptr(old_head.as_ref() as *const BufPage)
            };
            unsafe { cursor.insert_after(UnsafeRef::from_raw(buf_page)) };
        }

        // **Always** mark it as in the list:
        self.mark_page_in_lru_list(buf_page);

        // 2) Update our own LRU‐old pointers/timestamps
        //    No pool‐lock or upgrade() here! We already have everything
        //    we need in `self` (the BufLRU instance).
        if length > OLD_MIN_LENGTH {
            // once we exceed the threshold, we must have an old‐head
            ut_ad!(self.lru_old_head.is_some());
            self.buf_page_set(buf_page, old);
        } else if length < OLD_MIN_LENGTH {
            // still building up: use whichever region we’ve already marked
            self.buf_page_set(buf_page, self.lru_old_head.is_some());
        }
    }

    /// Mark the page as part of lru list and incrementing the lru_size.
    fn mark_page_in_lru_list(&mut self, buf_page: &mut BufPage) {
        buf_page.set_is_in_lru_list(true);
        self.lru_size += 1;
    }

    /// Mark the page as NOT a part of lru list and decrementing the lru_size.
    fn mark_page_not_in_lru_list(&mut self, buf_page: &mut BufPage) {
        self.lru_size -= 1;
        buf_page.set_is_in_lru_list(false);
    }

    /// Sets the 'old' status of a buffer page.
    ///
    /// This method marks a buffer page as either 'old' or 'new' in the LRU list.
    /// Pages marked as 'old' are candidates for eviction before 'new' pages.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to update
    /// * `old` - Whether to mark the page as old (true) or new (false)
    ///
    /// # Panics
    ///
    /// Panics if the page is not in a file or not in the LRU list
    ///
    fn buf_page_set(&mut self, buf_page: &mut BufPage, old: bool) {
        ut_a!(buf_page.in_file());
        ut_ad!(buf_page.is_in_lru_list());

        buf_page.set_old(old);
    }

    /// Set a new `old_ratio_pct` (0–100) and compute which pages flip.
    ///
    /// This only knows about OLD_RATIO_DIV, TOLERANCE, etc.
    ///
    /// # Returns
    ///
    /// `Vec<PageId>` - A vector of `PageId`s of pages whose old bit flipped
    ///
    pub(crate) fn set_old_ratio_pct_and_collect(
        &mut self,
        old_pct: usize,
    ) -> Vec<PageId> {
        let mut ratio = (old_pct * OLD_RATIO_DIV) / 100;
        ratio = ratio.clamp(OLD_RATIO_MIN, OLD_RATIO_MAX);
        // reuse our existing `old_ratio_update_list` logic by passing
        // that fixed‐point ratio directly:
        self.old_ratio_update_list(ratio)
    }

    /// Adjusts `old_ratio`, moves the `lru_old_head` pointer in/out by up to
    /// `OLD_TOLERANCE`, updates `lru_old_len`, and returns the list of pages
    /// whose “old” bit flipped.
    fn old_ratio_update_list(&mut self, new_ratio: usize) -> Vec<PageId> {
        ut_ad!((OLD_RATIO_MIN..=OLD_RATIO_MAX).contains(&new_ratio));

        // 1) If unchanged, nothing to do
        if new_ratio == self.old_ratio {
            return Vec::new();
        }
        self.old_ratio = new_ratio;
        // Take ownership of the head pointer so we can reuse it without borrowing `self`:
        let mut head_ref =
            self.lru_old_head.take().expect("there must be an old‐head");

        let mut old_len = self.lru_old_len;

        let mut changed = Vec::new();
        let lru_len = self.lru_size;
        let target = min(
            lru_len * new_ratio / OLD_RATIO_DIV,
            lru_len - OLD_TOLERANCE - NON_MIN_LENGTH,
        );

        // Grow the “old” region
        while old_len + OLD_TOLERANCE < target {
            let ptr = head_ref.deref();
            let mut cursor = unsafe { self.lru_list.cursor_from_ptr(ptr) };
            cursor.move_prev();
            let new_head = cursor.get().unwrap();
            head_ref = unsafe { UnsafeRef::from_raw(new_head) };
            old_len += 1;
            changed.push(new_head.get_page_id());
        }

        // Shrink the “old” region
        while old_len > target + OLD_TOLERANCE {
            let ptr = head_ref.deref();
            let mut cursor = unsafe { self.lru_list.cursor_from_ptr(ptr) };
            cursor.move_next();
            let new_head = cursor.get().unwrap();
            head_ref = unsafe { UnsafeRef::from_raw(new_head) };
            old_len -= 1;
            changed.push(new_head.get_page_id());
        }
        self.lru_old_head = Some(head_ref);
        self.lru_old_len = old_len;

        changed
    }

    /// Gets a free block from the buffer pool's free list without modifying the LRU list.
    ///
    /// This method attempts to retrieve a block directly from the free list
    /// without moving blocks from the LRU list if the free list is empty.
    ///
    /// # Returns
    ///
    /// * `Some(BufBlock)` - A free buffer block if available on the free list
    /// * `None` - If the free list is empty
    ///
    pub fn get_free_only(&mut self) -> Option<Arc<RwLock<BufBlock>>> {
        let buf_page = self.remove_from_free_list_top();

        if let Some(buf_page) = buf_page {
            let buf_block = buf_page.buf_page_get_buf_block();
            if let Some(block) = buf_block.upgrade().clone() {
                let mut block_ref = block.write().unwrap();

                {
                    let buf_page_ref = block_ref.buf_block_get_page_mut();

                    ut_ad!(!buf_page_ref.is_in_lru_list());
                    ut_ad!(!buf_page_ref.is_in_flush_list());
                    ut_ad!(!buf_page_ref.in_file());
                }

                block_ref.buf_block_set_state(BufBlockReadyForUse);
                block_ref.get_page().page_reset();

                return Some(block.clone());
            }

            ut_panic!("cannot set state for buf_block");
        }

        None
    }

    /// Adds a buffer page to the free list.
    ///
    /// This method adds a buffer page to the free list, making it available for
    /// reuse. It also updates the page's status to indicate it is now in the LRU list.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to add to the free list
    ///
    pub fn add_to_free_list(&mut self, buf_page: &mut BufPage) {
        ut_ad!(!buf_page.is_in_free_list());
        let free_list_ref = &mut self.free_list;
        unsafe {
            free_list_ref.push_back(UnsafeRef::from_raw(buf_page));
        }

        self.mark_page_as_part_of_free_list(buf_page);
    }

    /// Adds a buffer page to the free list from FRONT.
    ///
    /// This method adds a buffer page to the free list, making it available for
    /// reuse. It also updates the page's status to indicate it is now in the LRU list.
    ///
    /// # Arguments
    /// * `buf_page` - The buffer page to add to the free list
    pub fn add_to_free_list_front(&mut self, buf_page: &mut BufPage) {
        ut_ad!(!buf_page.is_in_free_list());
        let free_list_ref = &mut self.free_list;
        unsafe {
            free_list_ref.push_front(UnsafeRef::from_raw(buf_page));
        }
        self.mark_page_as_part_of_free_list(buf_page);
    }

    fn remove_from_free_list_top(&mut self) -> Option<UnsafeRef<BufPage>> {
        let page_ref = self.free_list.pop_front()?;

        // Clear the “in free list” flag under the block's write lock.
        if let Some(block) = page_ref.buf_page_get_buf_block().upgrade() {
            let mut guard = block.write().unwrap();
            self.mark_page_not_a_part_of_free_list(
                guard.buf_block_get_page_mut(),
            )
        }

        Some(page_ref)
    }

    /// Mark the page as a part of free list and incrementing the free list size by 1
    fn mark_page_not_a_part_of_free_list(&mut self, buf_page: &mut BufPage) {
        // Optional sanity:
        ut_a!(buf_page.is_in_free_list());
        self.free_list_size = self
            .free_list_size
            .checked_sub(1)
            .expect("free_list_size underflow");
        buf_page.set_is_in_free_list(false);
    }

    /// Mark the page as part of the free list and incrementing the free_list_size
    fn mark_page_as_part_of_free_list(&mut self, buf_page: &mut BufPage) {
        // Optional sanity:
        ut_a!(!buf_page.is_in_free_list());
        self.free_list_size = self
            .free_list_size
            .checked_add(1)
            .expect("free_list_size underflow");
        buf_page.set_is_in_free_list(true);
    }

    /// Removes a buffer page from the LRU list.
    ///
    /// This is a public wrapper around the internal `remove_block` method,
    /// providing external access to remove a page from the LRU list.
    ///
    /// # Arguments
    ///
    /// * `buf_page` - The buffer page to remove from the LRU list
    ///
    pub fn buf_page_remove_from_lru_list(&mut self, buf_page: &mut BufPage) {
        self.remove_block(buf_page);
    }

    /// Returns the number of blocks in the free list.
    ///
    /// # Returns
    /// * `usize` - The number of blocks in the free list
    pub fn get_free_list_size(&self) -> usize {
        self.free_list_size
    }

    /// This method returns the size of the lru list.
    pub fn get_lru_size(&self) -> usize {
        self.lru_size
    }
    
    
    

    /// Attempts to free a buffer block from the LRU list.
    ///
    /// This method checks if a buffer page can be freed based on its current state.
    /// It verifies that the page is in a file and in the LRU list and checks
    /// whether the page can be relocated and if it has been modified.
    ///
    /// Evict *this* page from the LRU *and* the hash, bump both clocks,
    /// wipe its on-disk header, and push it onto the free-list.
    ///
    /// # Returns
    ///
    /// `true` if the block was successfully freed, `false` otherwise.
    fn free_block(
        &mut self,
        pool_inner: &mut BufPoolInner,
        _stats: &Arc<BufPoolStats>,
        block: &mut RwLockWriteGuard<BufBlock>,
    ) -> bool {
        // 0) Eligibility
        {
            let bp = block.buf_block_get_page_mut();
            ut_a!(bp.in_file());
            ut_a!(bp.is_in_lru_list());
            ut_ad!(
                (bp.is_in_flush_list() && bp.buf_page_modified())
                    || (!bp.is_in_flush_list() && !bp.buf_page_modified())
            );
            if bp.buf_page_modified() || !bp.buf_page_can_relocate() {
                return false;
            }
        }

        // 1) Unlink from LRU (sets in_lru_list = false)
        {
            let buf_page = block.buf_block_get_page_mut();
            self.remove_block(buf_page);
        }

        // 2) Bump global freed-page clock
        pool_inner.set_freed_page_clock(
            pool_inner.freed_page_clock().wrapping_add(1),
        );

        // 3) Transient state: mark REMOVE_HASH and drop from hash using current pid
        block.buf_block_incr_clock();
        block.buf_block_set_state(BufPageState::BufBlockRemoveHash);
        {
            let buf_page = block.buf_block_get_page_mut();
            let pid = buf_page.get_page_id(); // capture before zapping
            pool_inner.page_hash_mut().remove(&pid);
            buf_page.set_is_in_hash_map(false);
            buf_page.set_is_in_flush_list(false);
        }

        // 4) Zap on-disk header and move to free list; final state = NOT_USED
        {
            let mut frame = block.get_page();
            frame.page_set_space_id(0xFF);
            frame.page_set_fil_offset(0xFF);

            block.buf_block_set_state(BufBlockNotUsed);

            let bp = block.buf_block_get_page_mut();
            self.add_to_free_list_front(bp);
        }

        true
    }

    /* 	Purpose: forcibly free a file-backed page when its tablespace has been truncated/dropped. No flushing.
    •	Preconditions: page is in file; it may be on LRU, hash, and/or flush-list; it may even be dirty or pinned — we ignore that because the file is gone.
    •	Actions:
    1.	Unlink from LRU if present.
    2.	Remove from the page hash.
    3.	Clear in_flush_list (and have BufFlush remove it from its intrusive flush list).
    4.	“Zap” header fields (space/page id) so it’s no longer “in file”.
    5.	Push onto the free list (front).
    6.	Transition the block’s state to BufBlockNotUsed.
    7.	Bump pool freed_page_clock and the block’s modify clock.
    •	Stats: unlike the non-file path, this should bump freed_page_clock (it’s an eviction).
    */
    pub(crate) fn free_block_truncated(
        &mut self,
        inner: &mut BufPoolInner,
        guard: &mut RwLockWriteGuard<BufBlock>,
    ) {
        // 1) unlink from LRU if needed
        if guard.buf_block_get_page().is_in_lru_list() {
            self.remove_block(guard.buf_block_get_page_mut());
        }
        // 2) remove from hash
        let pid = guard.buf_block_get_page().get_page_id();
        inner.page_hash_mut().remove(&pid);
        {
            let p = guard.buf_block_get_page_mut();
            p.set_is_in_hash_map(false);
            p.set_is_in_flush_list(false); // BufFlush should also unlink from its list
            // mark “not in file” by zapping header fields
            let mut frame = guard.get_page();
            frame.page_set_space_id(0xFF);
            frame.page_set_fil_offset(0xFF);
        }

        // 3) accounting
        inner.set_freed_page_clock(inner.freed_page_clock().wrapping_add(1));
        guard.buf_block_incr_clock();

        // 4) publish to free list + state
        self.add_to_free_list_front(guard.buf_block_get_page_mut());
        guard.buf_block_set_state(BufPageState::BufBlockNotUsed);
    }

    /// Frees a non-file-backed page and returns it to the buffer‐pool’s free list.
    ///
    /// This is used for blocks that do _not_ contain on‐disk pages (e.g. internal
    /// metadata blocks). It performs exactly the steps InnoDB’s
    /// `block_free_non_file_page` does:
    ///
    /// 1. **State check.** The block must be in one of:
    ///    - `BufPageState::BufBlockMemory`
    ///    - `BufPageState::BufBlockReadyForUse`
    ///      Otherwise this function panics.
    /// 2. **Mark `NOT_USED`.** Transitions the block state to `BufBlockNotUsed`.
    /// 3. **Wipe header.**  
    ///    - In debug builds: zeros out the _entire_ page.  
    ///    - In release builds: only overwrites the page‐id header fields.  
    /// 4. **Insert into the free list.** Pushes the page at the _front_ of the
    ///    free‐list and sets its `in_free_list` flag to `true`.
    ///
    /// # Arguments
    ///
    /// * `inner` — the locked‐pool data
    /// * `buf_block` — mutable reference to the buffer block to free
    ///
    pub(crate) fn block_free_non_file_page(
        &mut self,
        inner: &mut BufPoolInner,
        buf_block: &mut BufBlock,
    ) {
        // sanity: only MEMORY or READY_FOR_USE may be freed here
        match buf_block.buf_block_get_state() {
            BufPageState::BufBlockMemory | BufBlockReadyForUse => {}
            other => panic!("block_free_non_file_page: bad state {:?}", other),
        }
        // 1) Transition to NOT_USED
        buf_block.buf_block_set_state(BufBlockNotUsed);

        // 2) Wipe memory
        let mut page = buf_block.get_page();
        if cfg!(debug_assertions) {
            // wipe *entire* page if debugging
            page.page_set_fil_offset(0xFF);
        } else {
            // otherwise just zap the header fields
            page.page_set_fil_offset(0xFF);
            page.page_set_space_id(0xFF);
        }

        {
            let buf_page = buf_block.buf_block_get_page_mut();
            // sanity: must not yet be on any list
            ut_ad!(!buf_page.is_in_free_list());
            ut_ad!(!buf_page.is_in_flush_list());
            ut_ad!(!buf_page.is_in_lru_list());
            // 4) Push onto the “free” list
            self.add_to_free_list_front(buf_page)
        }
    }

    /// Low‐level removal of `buf_page` from the LRU **and** bump the pool‐inner clock.
    ///
    /// Caller must hold:
    ///  1) `pool_inner: &mut BufPoolInner` (already locked),
    ///  2) and the page’s RwLock write‐guard so that `buf_page` is safe to mutate.
    pub(crate) fn remove_page_from_lru(
        &mut self,
        pool_inner: &mut BufPoolInner,
        block_guard: &mut RwLockWriteGuard<BufBlock>,
    ) {
        // 0) Sanity checks
        let buf_page = block_guard.buf_block_get_page_mut();
        ut_a!(buf_page.buf_page_get_io_fix() == BufIoFix::BufIoNone);
        ut_a!(buf_page.buf_page_fix_count() == 0);
        buf_page.buf_page_get_io_fix();

        // 1) Unlink from our intrusive LRU list
        self.remove_block(buf_page);

        // 2) Bump the pool‐free clock
        pool_inner.set_freed_page_clock(
            pool_inner.freed_page_clock().wrapping_add(1),
        );

        // 3) Advance the BufBlock’s own “clock” if it’s a file page
        match buf_page.get_state() {
            BufPageState::BufBlockFilePage => {
                block_guard.buf_block_incr_clock();
            }
            _rest_of_states => {
                ut_panic!(
                    "buf_page should not be in state: {}",
                    _rest_of_states
                );
            }
        }
    }

    /// Attempts to evict (“free”) one cold page from the LRU list, scanning up to
    /// `n_iterations` frames before giving up.
    ///
    /// # Safety / Locking
    ///
    /// This helper **assumes** that the caller has already:
    /// 1. Locked the **BufPool** mutex, yielding a `&mut BufPool` reference.  
    /// 2. Locked the **BufLRU** mutex (i.e. has exclusive access to `self`).  
    ///
    /// By taking `pool: &mut BufPool` as an argument, the Rust type system
    /// ensures at compile time that the pool‐wide lock is held.  Inside, we
    /// only ever mutate global lists or stats via that `&mut BufPool`, and we
    /// acquire each per‐page latch (*after* these checks) to safely call
    /// `free_block(page_mut)`, which will unlink the page from the LRU on
    /// success.
    ///
    /// # Algorithm
    ///
    /// 1. Compute a “budget” = `100 + (n_iterations * pool.current_size()) / 10`.  
    /// 2. **Phase 1**: collect up to `budget` pointers (`UnsafeRef<BufPage>`) by
    ///    iterating the tail of the intrusive LRU list.  This only reads under
    ///    the pool lock and avoids any conflicting `&mut self` borrows.  
    /// 3. **Phase 2**: for each candidate:
    ///    - Assert that its `is_in_lru_list` and `in_file` flags are still true  
    ///      (safe because the pool lock is still held).  
    ///    - Lock the block’s latch (`write()`), then call `self.free_block(page_mut)`.  
    ///      On `BlockStatus::Freed` it will remove the node from the LRU and
    ///      publish it to the free list.  
    ///    - Drop the per‐block latch, update stats if needed, and return `true` on
    ///      success.  Otherwise continue to the next candidate.  
    ///
    /// Returns `true` if any page was evicted, else `false`.
    /// TODO this is not efficient yet because we are copying the candidates into the temp Vec.
    /// Need to figure out O(1) solution using intrusive list
    fn free_from_common_lru_list(
        &mut self,
        pool: &mut BufPoolInner,
        stats: &Arc<BufPoolStats>,
        n_iterations: usize,
    ) -> bool {
        // 1) Compute our scan budget
        let curr_size = pool.curr_size();
        let budget = 100 + (n_iterations * curr_size) / 10;

        // PHASE 1: collect up to `budget` cold-page *pointers*
        let mut candidates = Vec::with_capacity(budget);
        for page_ref in self.lru_list.iter().rev().take(budget) {
            candidates.push(page_ref.clone()); // just an UnsafeRef<BufPage> pointer
        }

        // PHASE 2: attempt eviction on each candidate in turn
        for buf_page in candidates {
            // Sanity checks (we still have pool lock)
            // These flags (`is_in_lru_list`, `in_file`)
            // are only ever mutated while the pool lock is held,
            // so reading them under the same lock is race-free:
            ut_a!(buf_page.is_in_lru_list());
            ut_a!(buf_page.in_file());

            let accessed = buf_page.buf_page_is_accessed();

            // Lock the actual block and call free_block
            let block_arc = buf_page
                .buf_page_get_buf_block()
                .upgrade()
                .expect("BufBlock was dropped");
            let mut block_guard = block_arc.write().unwrap();

            // free_block will unlink from the LRU list on FREED,
            // leave it in place on NOT_FREED
            if self.free_block(pool, stats, &mut block_guard) {
                // bump readahead-eviction stat _only_ if it was cold
                if accessed == 0 {
                    stats.n_ra_pages_evicted.fetch_add(1, Relaxed);
                }
                return true;
            }
        }

        // no page could be evicted in our budget
        false
    }

    /// Searches for a buffer block to free and removes it from the LRU list.
    ///
    /// This method scans the LRU list for a buffer block that can be freed,
    /// and if found, removes it from the list and returns it to the free list.
    /// It performs a limited number of iterations to avoid excessive scanning.
    ///
    /// # Arguments
    ///
    /// * `pool` - A mutable reference to the buffer pool inner state
    /// * `stats` - A reference to the buffer pool statistics
    /// * `n_iterations` - The maximum number of iterations to perform while searching
    ///
    /// # Returns
    ///
    /// * `bool` - `true` if a page was successfully freed, `false` otherwise
    ///
    /// # Safety
    ///
    /// Assumes the caller holds both the pool-wide lock (so `pool` is stable)
    /// and the LRU lock (so `self` is stable).
    ///
    /// // IMPORTANT READ:  Eviction vs flushing:
    /// Eviction reclaims frames but only from pages that are clean & unpinned.
    /// Flushing writes dirty pages to disk to *create* more clean candidates.
    /// Flow: need frame -> try free list -> try evict from LRU tail -> if tail is dirty/pinned,
    /// trigger a flush batch (LRU or flush-list) -> when batch ends, retry eviction.
    /// lru_flush_ended is a small hysteresis counter to avoid ping-pong: successful evictions
    /// count down; a no-progress eviction resets it to 0 so another flush can be requested.
    pub(crate) fn search_and_free_block(
        &mut self,
        pool: &mut BufPoolInner,
        stats: &Arc<BufPoolStats>,
        n_iterations: usize,
    ) -> bool {
        // 1) Try to free one page

        let freed = self.free_from_common_lru_list(pool, stats, n_iterations);

        if !freed {
            pool.set_lru_flush_ended(0)
        } else if pool.lru_flush_ended() > 0 {
            pool.set_lru_flush_ended(pool.lru_flush_ended() - 1);
        }

        freed
    }

    /// Gets the old ratio value.
    ///
    /// The old ratio determines what proportion of the buffer pool is reserved
    /// for "old" pages. It's expressed as a value between OLD_RATIO_MIN and OLD_RATIO_MAX,
    /// where the actual percentage is calculated as (old_ratio / OLD_RATIO_DIV) * 100.
    ///
    /// # Returns
    ///
    /// * `usize` - The current old ratio value
    ///
    pub fn old_ratio(&self) -> usize {
        self.old_ratio
    }

    pub fn old_threshold_ms(&self) -> u64 {
        self.old_threshold_ms
    }


    /// Gets a reference to the LRU list.
    ///
    /// The LRU (Least Recently Used) list contains all buffer pages that are
    /// currently in use, ordered by their recency of access. Pages at the front
    /// of the list are the most recently used, while pages at the end are candidates
    /// for eviction when the buffer pool needs to free space.
    ///
    /// # Returns
    ///
    /// * `&LinkedList<LruAdapter>` - A reference to the LRU list
    ///
    pub fn lru_list(&self) -> &LinkedList<LruAdapter> {
        &self.lru_list
    }

    pub(crate) fn lru_list_mut(&mut self) -> &mut LinkedList<LruAdapter> {
        &mut self.lru_list
    }

    /// Gets a reference to the LRU old head.
    ///
    /// The LRU old head is a pointer that divides the LRU list into two sections:
    /// the "new" section (before the pointer) and the "old" section (from the pointer
    /// to the end). Pages in the old section are candidates for early eviction.
    ///
    /// # Returns
    ///
    /// * `&Option<UnsafeRef<BufPage>>` - A reference to the LRU old head, or None if not set
    ///
    pub fn lru_old_head(&self) -> &Option<UnsafeRef<BufPage>> {
        &self.lru_old_head
    }

    /// Gets the length of the old section in the LRU list.
    ///
    /// This method returns the number of pages in the "old" section of the LRU list,
    /// which are candidates for early eviction when the buffer pool needs to free space.
    ///
    /// # Returns
    ///
    /// * `usize` - The number of pages in the old section of the LRU list
    ///
    pub fn lru_old_len(&self) -> usize {
        self.lru_old_len
    }

    /// Gets a reference to the free list.
    ///
    /// The free list contains buffer blocks that are not currently in use and are
    /// available for immediate allocation. When a page is evicted from the buffer pool,
    /// its block is added to the free list.
    ///
    /// # Returns
    ///
    /// * `&LinkedList<FreeListAdapter>` - A reference to the free list
    ///
    pub fn free_list(&self) -> &LinkedList<FreeListAdapter> {
        &self.free_list
    }

    /// Sets the old ratio value.
    ///
    /// This method updates the proportion of the buffer pool that is reserved
    /// for "old" pages. The value should be between OLD_RATIO_MIN and OLD_RATIO_MAX.
    ///
    /// # Arguments
    ///
    /// * `old_ratio` - The new old ratio value
    ///
    pub fn set_old_ratio(&mut self, old_ratio: usize) {
        self.old_ratio = old_ratio;
    }

    /// Sets the LRU list.
    ///
    /// This method replaces the current LRU list with a new one. It's typically
    /// called during initialization or when resetting the buffer pool.
    ///
    /// # Arguments
    ///
    /// * `lru_list` - The new LRU list
    ///
    pub fn set_lru_list(&mut self, lru_list: LinkedList<LruAdapter>) {
        self.lru_list = lru_list;
    }

    /// Sets the LRU old head.
    ///
    /// This method updates the pointer that divides the LRU list into "new" and "old"
    /// sections. It's called when adjusting the size of the old section based on
    /// the old ratio.
    ///
    /// # Arguments
    ///
    /// * `lru_old_head` - The new LRU old head pointer, or None to clear it
    ///
    pub fn set_lru_old_head(
        &mut self,
        lru_old_head: Option<UnsafeRef<BufPage>>,
    ) {
        self.lru_old_head = lru_old_head;
    }

    /// Sets the length of the old section in the LRU list.
    ///
    /// This method updates the count of pages in the "old" section of the LRU list.
    /// It's called when adjusting the size of the old section based on the old ratio.
    ///
    /// # Arguments
    ///
    /// * `lru_old_len` - The new length of the old section
    ///
    pub fn set_lru_old_len(&mut self, lru_old_len: usize) {
        self.lru_old_len = lru_old_len;
    }

    /// Sets the free list.
    ///
    /// This method replaces the current free list with a new one. It's typically
    /// called during initialization or when resetting the buffer pool.
    ///
    /// # Arguments
    ///
    /// * `free_list` - The new free list
    ///
    pub fn set_free_list(&mut self, free_list: LinkedList<FreeListAdapter>) {
        self.free_list = free_list;
    }

    // Clear the free list. Possibly used when we open the new bufpool.
    pub fn clear_free_list(&mut self) {
        self.free_list.clear();
        self.free_list_size = 0;
    }

    /// Compute how many pages to flush/evict so that we always have
    /// at least a “baseline” of ready frames plus a small cushion.
    ///
    /// 1. free_block_margin = 5 + read_ahead  
    ///    • read_ahead = min(next_power_of_two(curr_size/32), 64)  
    /// 2. extra_margin = free_block_margin/4 + 100  
    /// 3. Count how many frames are already free (free_list_len).  
    /// 4. Scan up to get_free_search_len() pages from the LRU tail,
    ///    adding 1 for each clean, unpinned page you could evict.  
    /// 5. If available >= free_block_margin, return 0.  
    ///    Otherwise return (free_block_margin + extra_margin) – available.
    ///
    pub(crate) fn recommendation(&self) -> usize {
        // 1) compute the two margins
        let read_ahead = min(ut_2_power_up!(self.lru_size / 32), 64);

        let free_block_margin = 5 + read_ahead as usize;
        let extra_margin = free_block_margin / 4 + 100;

        let mut n_replaceable = self.get_free_list_size();

        // 3) scan backwards through the LRU
        for page_ref in self
            .lru_list
            .iter()
            .rev()
            .take(self.get_free_search_len(self.lru_size))
        {
            // once we've seen enough candidates, stop
            if n_replaceable >= free_block_margin + extra_margin {
                break;
            }

            // upgrade to the block, grab its page‐level lock
            // grab the Weak<RwLock<BufBlock>>
            let weak_block: Weak<RwLock<BufBlock>> =
                page_ref.buf_page_get_buf_block();
            // upgrade it to Arc, or panic if it’s gone:
            let block: Arc<RwLock<BufBlock>> =
                weak_block.upgrade().expect("BufBlock vanished!");
            let guard = block.read().unwrap();
            let buf_page = guard.buf_block_get_page();

            if buf_page.in_file()
                && buf_page.buf_page_get_io_fix() == BufIoFix::BufIoNone
                && buf_page.buf_page_fix_count() == 0
                && !buf_page.buf_page_modified()
            {
                n_replaceable += 1;
            }
        }

        // 4) if we already meet the “free block margin”, nothing to do
        if n_replaceable >= free_block_margin {
            0
        } else {
            // otherwise tell the flusher how many to push through
            (free_block_margin + extra_margin) - n_replaceable
        }
    }

    /// How many pages deep into the LRU we will scan.
    /// Mirrors: 5 + 2 * read_ahead_area.
    pub(crate) fn get_free_search_len(&self, curr_size: usize) -> usize {
        // same read_ahead you use for free_block_margin:
        let read_ahead = min(ut_2_power_up!((curr_size / 32)), 64);
        5 + 2 * (read_ahead as usize)
    }
}
