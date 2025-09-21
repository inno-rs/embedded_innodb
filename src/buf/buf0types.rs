use std::{
    alloc::Layout,
    cmp::PartialEq,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use intrusive_collections::{LinkedListLink, UnsafeRef, intrusive_adapter};
use parking_lot::{RwLock as ParkingLotRwLock, lock_api::RawRwLock};
use strum_macros::Display;
use tracing::info;

use crate::{
    buf::{buf0flu::BufFlushType, buf0types::BufIoFix::BufIoNone},
    innodb::innodb0types::{
        Lsn, PageFrame, PageId, PageNo, SpaceId, UNIV_PAGE_SIZE,
    },
    mtr::mtr0mtr::Mtr,
    page::page0page::Page,
    sync::sync0rw::ReadWriteLatch,
    ut::{ut0dbg::ut_ad, ut0math::ut_align_down},
};

/// Magic number used to identify buffer pool pages that do not have a checksum.
pub const BUF_NO_CHECKSUM_MAGIC: usize = 0xDEADBEEF;

/// The frame is a pointer to a memory location where the block's data is stored.
/// It points to the actual page content in memory.
pub type Frame = *mut u8;

/// Modes for buffer pool page retrieval operations.
/// These constants define different strategies for retrieving pages from the buffer pool.
/// Always get the page, loading it from the disk if necessary.
/// This mode ensures the requested page is returned, even if it requires a disk read.
pub const BUF_GET_ALWAYS: usize = 10;

/// Only get the page if it's already in the buffer pool.
/// This mode avoids disk reads and returns None if the page isn't cached.
pub const BUF_GET_IF_IN_POOL: usize = 11;

/// Get the page and mark it as fixed, but don't set any latch on it.
/// This mode is used when the caller doesn't need exclusive access to the page.
pub const BUF_GET_NO_LATCH: usize = 14;

pub struct BufPoolStats {
    /// Number of gets performed.
    pub n_pages_get: AtomicU64,
    /// Number of read operations performed.
    pub n_pages_read: AtomicU64,
    /// Number of write operations performed.
    pub n_pages_written: AtomicU64,
    /// Number of pages created in the pool with no read.
    pub n_pages_created: AtomicU64,
    /// Number of pages read as part of read ahead.
    pub n_ra_pages_read: AtomicU64,
    /// Number of read ahead pages that were evicted without being read.
    pub n_ra_pages_evicted: AtomicU64,
    /// Number of pages made young.
    pub n_pages_made_young: AtomicU64,
    /// The Number of pages isn't made young because the first access was not long enough.
    pub n_pages_not_made_young: AtomicU64,
}

impl BufPoolStats {
    pub fn new() -> Self {
        Self {
            n_pages_get: AtomicU64::new(0),
            n_pages_read: AtomicU64::new(0),
            n_pages_written: AtomicU64::new(0),
            n_pages_created: AtomicU64::new(0),
            n_ra_pages_read: AtomicU64::new(0),
            n_ra_pages_evicted: AtomicU64::new(0),
            n_pages_made_young: AtomicU64::new(0),
            n_pages_not_made_young: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.n_pages_get.store(0, Ordering::Relaxed);
        self.n_pages_read.store(0, Ordering::Relaxed);
        self.n_pages_written.store(0, Ordering::Relaxed);
        self.n_pages_created.store(0, Ordering::Relaxed);
        self.n_ra_pages_read.store(0, Ordering::Relaxed);
        self.n_ra_pages_evicted.store(0, Ordering::Relaxed);
        self.n_pages_made_young.store(0, Ordering::Relaxed);
        self.n_pages_not_made_young.store(0, Ordering::Relaxed);
    }
}

/// Represents the I/O status of a buffer page.
///
/// This enum indicates whether a buffer page has any pending I/O operations
/// (read or write) or is free from I/O operations. It's used to track the
/// I/O state of pages in the buffer pool and prevent operations on pages
/// that are currently involved in I/O.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum BufIoFix {
    /// No pending I/O operations on this page.
    /// The page is not being read from or written to disk.
    BufIoNone = 0,
    /// A read operation is pending on this page.
    /// The page is currently being read from disk.
    BufIoRead = 1,
    /// A write operation is pending on this page.
    /// The page is currently being written to disk.
    BufIoWrite = 2,
}

/// Represents a page in the buffer pool.
///
/// BufPage contains metadata about a database page that is cached in the buffer pool.
/// It tracks the page's identity, state, modification history, and maintains links
/// for various buffer pool management lists (free, LRU, flush).
#[derive(Clone, Debug)]
pub struct BufPage {
    /// Table space identifier.
    space_id: SpaceId,
    /// Page identifier within the tablespace.
    page_no: PageNo,
    /// Current state of the buffer page.
    state: BufPageState,
    /// Indicates the type of flush_type.
    flush_type: BufFlushType,
    /// Type of pending i/o operation.
    io_fix: BufIoFix,
    /// Count of the number of current fixes.
    buf_fix_count: usize,
    /// True, the block is marked as old in the lru list. False, otherwise.
    old: bool,
    /// Timestamp of when this page was last put at the head of the LRU list.
    freed_page_lock: usize,
    /// Log Sequence Number of the youngest modification to the page. Zero, if the page is not modified.
    newest_modification: Lsn,
    /// Log Sequence Number of the oldest modification which has not yet been flushed to disk.
    /// Zero, if the page is not modified.
    oldest_modification: Lsn,
    /// Link for the free list of buffer pages.
    free_list: LinkedListLink,
    /// Link for the flush list of modified buffer pages.
    flush_list: LinkedListLink,
    /// Link for the LRU (Least Recently Used) list of buffer pages.
    lru_list: LinkedListLink,
    /// Indicated if the page is currently in hashmap.
    is_in_hash_map: bool,
    /// Indicates if the page is currently in the free list.
    is_in_free_list: bool,
    /// Indicates if the page is currently in the flush list.
    is_in_flush_list: bool,
    /// Indicates if the page is currently in the LRU list.
    is_in_lru_list: bool,
    ///this is set to true when fsp frees a page in buffer pool
    file_page_was_freed: bool,

    /// Timestamp (ms) of the **first** time this page was accessed (0 = not yet set).
    access_time: u64,

    buf_block: Weak<RwLock<BufBlock>>,
    /// snapshot of pool->freed_page_clock
    freed_page_clock: usize,
}

// Adapter for intrusive collections to manage flush lists of buffer pages.
intrusive_adapter!(pub FlushListAdapter = UnsafeRef<BufPage>: BufPage { flush_list: LinkedListLink });
// Adapter for intrusive collections to manage free lists of buffer pages.
intrusive_adapter!(pub FreeListAdapter = UnsafeRef<BufPage>: BufPage { free_list: LinkedListLink });
// Adapter for intrusive collections to manage LRU lists of buffer pages.
intrusive_adapter!(pub LruAdapter = UnsafeRef<BufPage>: BufPage { lru_list: LinkedListLink });

impl BufPage {
    /// Creates a new buffer page with the specified tablespace ID and page number.
    ///
    /// # Arguments
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    ///
    /// # Returns
    /// A new BufPage instance initialized with default values
    pub fn new(space_id: SpaceId, page_no: PageNo) -> Self {
        BufPage {
            space_id,
            page_no,
            state: BufPageState::BufBlockNotUsed,
            flush_type: BufFlushType::None,
            io_fix: BufIoNone,
            buf_fix_count: 0,
            old: false,
            freed_page_lock: 0,
            newest_modification: 0,
            oldest_modification: 0,
            free_list: Default::default(),
            flush_list: Default::default(),
            lru_list: Default::default(),
            is_in_hash_map: false,
            is_in_free_list: false,
            is_in_flush_list: false,
            is_in_lru_list: false,
            buf_block: Default::default(),
            access_time: 0,
            file_page_was_freed: false,
            freed_page_clock: 0usize,
        }
    }

    /// Gets the page ID of this buffer page.
    ///
    /// This method creates and returns a PageId struct that uniquely identifies
    /// this page within the database, combining the space ID and page number.
    ///
    /// # Returns
    /// * `PageId` - The unique identifier for this page
    pub fn get_page_id(&self) -> PageId {
        PageId { space_id: self.space_id, page_no: self.page_no }
    }

    /// Sets a reference to the buffer block that contains this page.
    ///
    /// This method establishes the connection between a buffer page and its containing buffer block.
    ///
    /// # Arguments
    /// * `buf_block` - A weak reference to the buffer block containing this page
    pub fn set_buf_block_ref(&mut self, buf_block: Weak<RwLock<BufBlock>>) {
        self.buf_block = buf_block;
    }

    /// Creates a dummy buffer page with zero values for testing or placeholder purposes.
    ///
    /// # Returns
    /// A new BufPage instance with space_id and page_no set to 0
    pub fn dummy() -> Self {
        BufPage::new(0, 0)
    }

    /// Gets the log sequence number of the most recent modification to this page.
    ///
    /// # Returns
    /// The LSN of the newest modification, or 0 if the page is not modified
    pub fn get_newest_modification(&self) -> Lsn {
        self.newest_modification
    }

    /// Sets the log sequence number of the most recent modification to this page.
    ///
    /// # Arguments
    /// * `lsn` - The LSN to set as the newest modification
    pub fn set_newest_modification(&mut self, lsn: Lsn) {
        self.newest_modification = lsn;
    }

    /// Gets the log sequence number of the oldest unflushed modification to this page.
    ///
    /// # Returns
    /// The LSN of the oldest unflushed modification, or 0 if the page is not modified
    pub fn get_oldest_modification(&self) -> Lsn {
        self.oldest_modification
    }

    /// Sets the log sequence number of the oldest unflushed modification to this page.
    ///
    /// # Arguments
    /// * `lsn` - The LSN to set as the oldest modification
    pub fn set_oldest_modification(&mut self, lsn: Lsn) {
        self.oldest_modification = lsn;
    }

    /// Gets the current state of the buffer page.
    ///
    /// # Returns
    /// The current state of the buffer page
    pub fn get_state(&self) -> BufPageState {
        self.state.clone()
    }

    /// Checks if the buffer page is in the free list.
    ///
    /// A page is considered to be in the free list if its state is BufBlockNotUsed.
    ///
    /// # Returns
    /// * `true` if the page is in the free list, `false` otherwise
    pub fn is_in_free_list(&self) -> bool {
        self.is_in_free_list
    }

    /// Sets whether the buffer page is in the LRU list.
    ///
    /// # Arguments
    /// * `is_in_lru_list` - Whether the page is in the LRU list
    pub fn set_is_in_lru_list(&mut self, is_in_lru_list: bool) {
        self.is_in_lru_list = is_in_lru_list;
    }

    /// Sets whether the buffer page is in the flush list.
    ///
    /// # Arguments
    /// * `is_in_flush_list` - Whether the page is in the flush list
    pub fn set_is_in_flush_list(&mut self, is_in_flush_list: bool) {
        self.is_in_flush_list = is_in_flush_list;
    }

    /// Sets whether the buffer page is in the free list.
    ///
    /// # Arguments
    /// * `is_in_free_list` - Whether the page is in the free list
    pub fn set_is_in_free_list(&mut self, is_in_free_list: bool) {
        self.is_in_free_list = is_in_free_list;
    }

    /// Checks if the buffer page is in the flush list.
    ///
    /// Pages in the flush list are modified and need to be written to disk.
    ///
    /// # Returns
    /// * `true` if the page is in the flush list, `false` otherwise
    pub fn is_in_flush_list(&self) -> bool {
        self.is_in_flush_list
    }

    /// Checks if the buffer page is in the LRU list.
    ///
    /// # Returns
    /// * `true` if the page is in the LRU list, `false` otherwise
    pub fn is_in_lru_list(&self) -> bool {
        self.is_in_lru_list
    }

    /// Checks if the buffer page is in the hash map.
    ///
    /// # Returns
    /// * `true` if the page is in the hash map, `false` otherwise
    pub fn is_in_hash_map(&self) -> bool {
        self.is_in_hash_map
    }

    /// Sets whether the buffer page is in the hash map.
    ///
    /// # Arguments
    /// * `is_in_hash_map` - Whether the page is in the hash map
    pub fn set_is_in_hash_map(&mut self, is_in_hash_map: bool) {
        self.is_in_hash_map = is_in_hash_map;
    }

    /// Sets whether the buffer page is considered 'old' in the LRU list.
    ///
    /// Pages marked as 'old' are candidates for early eviction from the buffer pool.
    ///
    /// # Arguments
    /// * `old` - Whether to mark the page as old
    pub fn set_old(&mut self, old: bool) {
        self.old = old;
    }

    pub fn is_old(&self) -> bool {
        self.old
    }

    /// Checks if the buffer page is mapped to a tablespace file.
    ///
    /// This method determines whether the buffer page represents a page in a tablespace file
    /// (as opposed to a memory-only page or an unused page). Only pages in the BufBlockFilePage
    /// state are considered to be mapped to a file.
    ///
    /// # Returns
    /// * `true` - If the page is mapped to a tablespace file
    /// * `false` - If the page is not mapped to a file (e.g., it's a memory page or unused)
    pub fn in_file(&self) -> bool {
        matches!(self.state.clone(), BufPageState::BufBlockFilePage)
    }

    #[cfg(test)]
    pub fn set_in_file(&mut self) {
        self.state = BufPageState::BufBlockFilePage;
    }

    /// Checks if the buffer page has been modified and not yet flushed to disk.
    ///
    /// A page is considered modified if it has an oldest modification LSN greater than zero.
    /// Modified pages need to be written back to disk eventually.
    ///
    /// # Returns
    /// * `true` if the page has been modified, `false` otherwise
    pub fn buf_page_modified(&self) -> bool {
        self.oldest_modification > 0
    }

    /// Gets the current I/O fix status of the buffer page.
    ///
    /// This method returns the current I/O status, indicating whether the page
    /// has any pending read or write operations.
    ///
    /// # Returns
    /// * `BufIoFix` - The current I/O fix status of the page
    pub fn buf_page_get_io_fix(&self) -> BufIoFix {
        self.io_fix.clone()
    }

    /// Sets the I/O fix status of the buffer page.
    ///
    /// This method updates the I/O status of the page, indicating whether it has
    /// pending read or write operations. This affects how the page can be accessed
    /// and whether it can be evicted from the buffer pool.
    ///
    /// # Arguments
    /// * `io_fix` - The new I/O fix status to set for the page
    pub fn buf_page_set_io_fix(&mut self, io_fix: BufIoFix) {
        self.io_fix = io_fix.clone();
        ut_ad!(self.buf_page_get_io_fix() == io_fix);
    }

    /// Increments the buffer fix count for this page.
    ///
    /// This method increases the buffer fix count, which tracks how many operations
    /// are currently accessing this page. A page with a non-zero fix count cannot
    /// be evicted from the buffer pool. This is used to ensure that pages being
    /// actively used remain in memory.
    ///
    /// # Returns
    /// * `usize` - The new buffer fix count after incrementing
    pub fn buf_block_buf_fix_inc(&mut self) -> usize {
        self.buf_fix_count += 1;
        self.buf_fix_count
    }

    /// Gets the current flush type of the buffer page.
    ///
    /// # Returns
    /// The current flush type of the buffer page
    pub fn buf_page_get_flush_type(&self) -> BufFlushType {
        self.flush_type
    }

    /// Sets the flush type of the buffer page.
    ///
    /// This method specifies how the page should be flushed to disk when it's time
    /// to write it back. Different flush types (LRU, List, Single Page) correspond to
    /// different flushing strategies and priorities.
    ///
    /// # Arguments
    /// * `flush_type` - The new flush type to set for the page
    pub fn buf_page_set_flush_type(&mut self, flush_type: BufFlushType) {
        self.flush_type = flush_type;
        ut_ad!(self.buf_page_get_flush_type() == flush_type);
    }

    /// Gets the current buffer fix count of the page.
    ///
    /// The buffer fix count indicates how many operations are currently
    /// accessing this page. A page with a non-zero fix count cannot be evicted.
    ///
    /// # Returns
    /// * `usize` - The current buffer fix count
    pub fn buf_page_io_fix_count(&self) -> usize {
        self.buf_fix_count
    }

    #[cfg(test)]
    pub fn set_buf_page_io_fix_count(&mut self, count: usize) {
        self.buf_fix_count = count
    }

    /// Gets the space ID of the tablespace containing this page.
    ///
    /// # Returns
    /// * `SpaceId` - The ID of the tablespace containing this page
    pub fn get_space_id(&self) -> SpaceId {
        self.space_id
    }

    pub fn set_space_id(&mut self, space_id: SpaceId) {
        self.space_id = space_id;
    }

    /// Gets the page number of this page within its tablespace.
    ///
    /// # Returns
    /// * `PageNo` - The page number within the tablespace
    pub fn get_page_no(&self) -> PageNo {
        self.page_no
    }

    pub fn set_page_no(&mut self, page_no: PageNo) {
        self.page_no = page_no;
    }

    /// Gets a weak reference to the buffer block containing this page.
    ///
    /// This method returns a weak reference to avoid reference cycles between
    /// the buffer page and its containing buffer block.
    ///
    /// # Returns
    /// * `Weak<RefCell<BufBlock>>` - A weak reference to the buffer block
    pub fn buf_page_get_buf_block(&self) -> Weak<RwLock<BufBlock>> {
        Weak::clone(&self.buf_block)
    }

    /// Checks if the buffer page can be safely relocated in memory.
    ///
    /// This method determines if a page can be moved or evicted from the buffer pool.
    /// A page can be relocated if it has no pending I/O operations (io_fix is BufIoNone)
    /// and no active users (buf_fix_count is 0). The page can be dirty (modified),
    /// but must be in a file and in the LRU list.
    ///
    /// # Returns
    /// * `true` if the page can be relocated, `false` otherwise
    ///
    /// # Panics
    /// Panics in debug mode if the page is not in a file or not in the LRU list
    pub fn buf_page_can_relocate(&self) -> bool {
        ut_ad!(self.in_file());
        ut_ad!(self.is_in_lru_list());

        self.io_fix == BufIoNone && self.buf_fix_count == 0
    }

    /// Returns the (first) access timestamp, or 0 if never accessed yet.
    pub fn access_time(&self) -> u64 {
        self.access_time
    }

    /// Record the first access time (in ms). Subsequent calls do nothing.
    ///
    /// Panics if the page isn’t “in_file” (i.e. not on disk yet).
    pub fn set_accessed(&mut self, time_ms: u64) {
        // ut_a(bpage->in_file());
        assert!(
            self.in_file(),
            "set_accessed: page must be in_file before recording access"
        );

        if self.access_time == 0 {
            // first-ever access
            self.access_time = time_ms;
        }
    }

    pub fn buf_page_is_accessed(&self) -> u64 {
        assert!(self.in_file(), "buf_page_is_accessed: page must be in_file");
        self.access_time
    }

    pub fn set_file_page_was_freed(&mut self, file_page_was_freed: bool) {
        self.file_page_was_freed = file_page_was_freed;
    }

    pub fn freed_page_clock(&self) -> usize {
        self.freed_page_clock
    }

    pub fn set_freed_page_clock(&mut self, c: usize) {
        self.freed_page_clock = c;
    }
}

/// The buffer control block structure.
///
/// BufBlock represents a buffer block in the buffer pool, which contains both
/// the metadata about a page (BufPage) and a pointer to the actual page data in memory.
/// It also includes a lock for concurrency control.
pub struct BufBlock {
    /// Counter that tracks modifications to this buffer block.
    /// Incremented each time the block is removed from the hash table.
    modify_clock: AtomicUsize,
    /// Metadata about the page stored in this buffer block
    buf_page: BufPage,
    /// Pointer to the actual page data in memory
    frame: Frame,
    /// Lock for concurrency control when accessing this buffer block
    rw_lock: Arc<ParkingLotRwLock<()>>,
}

impl BufBlock {
    /// Creates a new buffer block with the specified buffer page and frame.
    ///
    /// # Arguments
    /// * `buf_page` - The buffer page metadata
    /// * `frame` - Pointer to the memory location where the page data is stored
    ///
    /// # Returns
    /// A new BufBlock instance
    pub fn new(buf_page: BufPage, frame: Frame) -> Self {
        ut_ad!(
            ut_align_down(frame as usize, UNIV_PAGE_SIZE) == frame as usize
        );

        BufBlock {
            modify_clock: AtomicUsize::new(0),
            buf_page,
            frame,
            rw_lock: Arc::new(Default::default()),
        }
    }

    /// Checks if the buffer block is latched in exclusive mode.
    ///
    /// # Returns
    ///
    /// * `true` if the block is latched in exclusive mode, `false` otherwise
    ///
    pub fn is_frame_x_latched(&self) -> bool {
        self.rw_lock.is_locked_exclusive()
    }

    /// Checks if the buffer block is latched in shared mode.
    ///
    /// # Returns
    ///
    /// * `true` if the block is latched in shared mode, `false` otherwise
    ///
    pub fn is_frame_s_latched(&self) -> bool {
        !self.is_frame_x_latched() && self.rw_lock.is_locked() /* is_locked only tells if it is locked in any form */
    }

    /// X latch the page frame.
    pub fn frame_x_latch(&self) {
        unsafe { self.rw_lock.raw().lock_exclusive() };
    }

    /// S latch the page frame.
    pub fn frame_s_latch(&self) {
        unsafe { self.rw_lock.raw().lock_shared() };
    }

    /// Gets the frame pointer for this buffer block.
    ///
    /// # Returns
    /// A pointer to the memory location where the page data is stored
    pub fn get_frame(&self) -> Frame {
        self.frame
    }

    pub fn get_frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }

    /// Gets the current state of the buffer page.
    ///
    /// # Returns
    /// The current state of the buffer page
    pub fn buf_block_get_state(&self) -> BufPageState {
        self.buf_page.state.clone()
    }

    /// Sets the state of the buffer page.
    ///
    /// This method updates the state of the buffer page, which affects how the page
    /// is managed in the buffer pool (e.g., whether it's in the free list, LRU list, etc.).
    /// The state transition may trigger additional actions such as adding or removing
    /// the page from various buffer pool management lists.
    ///
    /// # Arguments
    /// * `state` - The new state to set for the buffer page
    ///
    /// # Note
    /// This method is currently a placeholder and needs to be implemented.
    pub fn buf_block_set_state(&mut self, state: BufPageState) {
        self.buf_page.state = state;
    }

    /// Gets the buffer page metadata for this buffer block.
    ///
    /// # Returns
    /// A clone of the buffer page metadata
    pub fn buf_block_get_page(&self) -> &BufPage {
        &self.buf_page
    }

    /// Gets the page identifier for this buffer block.
    ///
    /// This method provides a convenient way to access the page ID (space ID and page number)
    /// of the page stored in this buffer block.
    ///
    /// # Returns
    /// The page identifier (PageId) of the page in this buffer block
    pub fn get_page_id(&self) -> PageId {
        self.buf_block_get_page().get_page_id()
    }

    /// Gets the buffer page metadata for this buffer block.
    ///
    /// # Returns
    /// A clone of the buffer page metadata
    pub fn buf_block_get_page_mut(&mut self) -> &mut BufPage {
        &mut self.buf_page
    }

    /// Increments the modification clock of this buffer block.
    ///
    /// This method is called when the block is removed from the hash table,
    /// typically during page eviction or relocation. The modification clock
    /// helps track how many times the block has been recycled.
    pub fn buf_block_incr_clock(&self) {
        self.modify_clock.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the current value of the modification clock.
    pub(crate) fn buf_block_get_clock(&self) -> usize {
        self.modify_clock.load(Ordering::Relaxed)
    }

    /// Gets a Page object that provides methods for accessing the page data.
    ///
    /// # Returns
    /// A Page object initialized with this buffer block's frame
    pub fn get_page(&self) -> Page {
        Page::new(self.frame)
    }

    /// Gets a PageFrame object that provides low-level access to the page data.
    ///
    /// This method creates a PageFrame wrapper around the raw memory buffer of the page.
    /// It allows for direct manipulation of the page's memory layout.
    ///
    /// # Returns
    /// A PageFrame object that provides access to the raw page data
    ///
    /// # Safety
    /// This method uses unsafe code to create a mutable reference to the page's memory.
    /// The caller must ensure that no other references to this memory exist while the
    /// PageFrame is in use.
    pub fn get_page_frame(&self) -> PageFrame<'_> {
        unsafe {
            PageFrame::new(&mut *(self.frame as *mut [u8; UNIV_PAGE_SIZE]))
        }
    }
    /// Grab the block’s write‐lock, then decrement its buf_fix_count.
    pub fn fix_dec(&mut self) {
        // Mutate the inner BufPage’s fix count
        let count = self
            .buf_page
            .buf_fix_count
            .checked_sub(1)
            .expect("unbalanced fix_dec");
        self.buf_page.buf_fix_count = count;
        // guard drops here, releasing the lock
    }
}

/// A request for retrieving a page from the buffer pool.
///
/// This struct encapsulates all the parameters needed to request a page
/// from the buffer pool, including the page identity, latch mode, and
/// the mini-transaction context.
pub struct Request<'a> {
    /// The type of latch to acquire on the page (shared, exclusive, or none)
    pub rw_latch: ReadWriteLatch,
    /// The unique identifier of the page to retrieve
    pub page_id: PageId,
    /// The mode of retrieval (e.g., BUF_GET_ALWAYS, BUF_GET_IF_IN_POOL)
    pub mode: usize,
    /// The mini-transaction context for logging and recovery
    pub mtr: &'a mut Mtr,
}

/// Represents the possible states of a buffer page.
///
/// This enum defines the lifecycle states of a buffer page as it moves
/// through the buffer pool management system.
#[derive(Clone, Eq, PartialEq, Debug, Display)]
pub enum BufPageState {
    /// The buffer block is not currently in use and is in the free list.
    BufBlockNotUsed,
    /// The buffer block contains a file page that is being cached.
    BufBlockFilePage,
    /// The buffer block has been allocated from the LRU list and is ready for use.
    /// This is a transitional state before it becomes a file page or memory page.
    BufBlockReadyForUse,
    /// The buffer block contains a main memory page (not a file page).
    /// These pages are used for internal data structures.
    BufBlockMemory,
    /// The buffer block is marked for hash index removal before being
    /// returned to the free list. This is a transitional state.
    BufBlockRemoveHash,
}

/// Represents a chunk of memory that contains multiple buffer pages.
///
/// A BufChunk is a contiguous block of memory that is allocated to store multiple
/// buffer pages. It manages the lifecycle of this memory block and the buffer pages
/// contained within it.
pub struct BufChunk {
    /// The memory layout used for allocation and deallocation
    pub layout: Layout,
    /// The total size of the memory block in bytes
    pub mem_size: usize,
    /// The number of buffer pages that can fit in this chunk
    pub size: usize,
    /// Pointer to the allocated memory block
    pub mem: *mut u8,
    /// Vector of buffer blocks contained in this chunk
    pub buf_pages: Vec<Arc<RwLock<BufBlock>>>,
}

/// Implements the Drop trait for BufChunk to ensure proper memory deallocation.
///
/// When a BufChunk is dropped, this implementation ensures that the memory
/// allocated for the chunk is properly deallocated, preventing memory leaks.
impl Drop for BufChunk {
    fn drop(&mut self) {
        info!(
            "releasing BufChunk with page: {}, mem_size: {}",
            self.size, self.mem_size
        );
        ut_ad!(!self.mem.is_null());
        unsafe {
            std::alloc::dealloc(self.mem, self.layout);
        }
    }
}
