use crate::buf::buf0flu::BufFlushType;
use crate::buf::buf0types::BufIoFix::BufIoNone;
use crate::innodb::innodb0types::{
    Lsn, PageId, PageNo, SpaceId, UNIV_PAGE_SIZE,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::page::page0page::Page;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use intrusive_collections::{LinkedListLink, UnsafeRef, intrusive_adapter};
use parking_lot::RwLock;
use std::cell::RefCell;
use std::cmp::PartialEq;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use strum_macros::Display;

/// Magic number used to identify buffer pool pages that do not have a checksum.
pub const BUF_NO_CHECKSUM_MAGIC: usize = 0xDEADBEEF;

/// The frame is a pointer to a memory location where the block's data is stored.
/// It points to the actual page content in memory.
pub type Frame = *mut u8;

/// Modes for buffer pool page retrieval operations.
/// These constants define different strategies for retrieving pages from the buffer pool.

/// Always get the page, loading it from disk if necessary.
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
    pub n_pages_get: u64,
    /// Number of read operations performed.
    pub n_pages_read: u64,
    /// Number of write operations performed.
    pub n_pages_written: u64,
    /// Number of pages created in the pool with no read.
    pub n_pages_created: u64,
    /// Number of pages read as part of read ahead.
    pub n_ra_pages_read: u64,
    /// Number of read ahead pages that were evicted without being read.
    pub n_ra_pages_evicted: u64,
    /// Number of pages made young.
    pub n_pages_made_young: u64,
    /// The Number of pages isn't made young because the first access was not long enough.
    pub n_pages_not_made_young: u64,
}

impl BufPoolStats {
    pub fn new() -> Self {
        Self {
            n_pages_get: 0,
            n_pages_read: 0,
            n_pages_written: 0,
            n_pages_created: 0,
            n_ra_pages_read: 0,
            n_ra_pages_evicted: 0,
            n_pages_made_young: 0,
            n_pages_not_made_young: 0,
        }
    }
}

/// Represents the I/O status of a buffer page.
///
/// This enum indicates whether a buffer page has any pending I/O operations
/// (read or write) or is free from I/O operations. It's used to track the
/// I/O state of pages in the buffer pool and prevent operations on pages
/// that are currently involved in I/O.
#[derive(Clone, Eq, PartialEq)]
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
#[derive(Clone)]
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

    buf_block: Weak<RefCell<BufBlock>>,
}

/// Adapter for intrusive collections to manage flush lists of buffer pages.
intrusive_adapter!(pub FlushListAdapter = UnsafeRef<BufPage>: BufPage { flush_list: LinkedListLink });
/// Adapter for intrusive collections to manage free lists of buffer pages.
intrusive_adapter!(pub FreeListAdapter = UnsafeRef<BufPage>: BufPage { free_list: LinkedListLink });
/// Adapter for intrusive collections to manage LRU lists of buffer pages.
intrusive_adapter!(pub LruAdapter = UnsafeRef<BufPage>: BufPage { lru_list: LinkedListLink });

impl BufPage {
    /// Creates a new buffer page with the specified tablespace ID and page number.
    ///
    /// # Parameters
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
    /// # Parameters
    /// * `buf_block` - A weak reference to the buffer block containing this page
    pub fn set_buf_block_ref(&mut self, buf_block: Weak<RefCell<BufBlock>>) {
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
    /// # Parameters
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
    /// # Parameters
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
        self.state == BufPageState::BufBlockNotUsed
    }

    /// Sets whether the buffer page is in the LRU list.
    ///
    /// # Parameters
    /// * `is_in_lru_list` - Whether the page is in the LRU list
    pub fn set_is_in_lru_list(&mut self, is_in_lru_list: bool) {
        self.is_in_lru_list = is_in_lru_list;
    }

    /// Sets whether the buffer page is in the flush list.
    ///
    /// # Parameters
    /// * `is_in_flush_list` - Whether the page is in the flush list
    pub fn set_is_in_flush_list(&mut self, is_in_flush_list: bool) {
        self.is_in_flush_list = is_in_flush_list;
    }

    /// Sets whether the buffer page is in the free list.
    ///
    /// # Parameters
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
        self.is_in_lru_list
    }

    /// Sets whether the buffer page is in the hash map.
    ///
    /// # Parameters
    /// * `is_in_hash_map` - Whether the page is in the hash map
    pub fn set_is_in_hash_map(&mut self, is_in_hash_map: bool) {
        self.is_in_hash_map = is_in_hash_map;
    }

    /// Sets whether the buffer page is considered 'old' in the LRU list.
    ///
    /// Pages marked as 'old' are candidates for early eviction from the buffer pool.
    ///
    /// # Parameters
    /// * `old` - Whether to mark the page as old
    pub fn set_old(&mut self, old: bool) {
        self.old = old;
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
        match self.state.clone() {
            BufPageState::BufBlockFilePage => true,
            _ => false,
        }
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
    /// # Parameters
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
        self.flush_type.clone()
    }

    /// Sets the flush type of the buffer page.
    ///
    /// This method specifies how the page should be flushed to disk when it's time
    /// to write it back. Different flush types (LRU, List, Single Page) correspond to
    /// different flushing strategies and priorities.
    ///
    /// # Parameters
    /// * `flush_type` - The new flush type to set for the page
    pub fn buf_page_set_flush_type(&mut self, flush_type: BufFlushType) {
        self.flush_type = flush_type.clone();
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

    /// Gets the space ID of the tablespace containing this page.
    ///
    /// # Returns
    /// * `SpaceId` - The ID of the tablespace containing this page
    pub fn get_space_id(&self) -> SpaceId {
        self.space_id
    }

    /// Gets the page number of this page within its tablespace.
    ///
    /// # Returns
    /// * `PageNo` - The page number within the tablespace
    pub fn get_page_no(&self) -> PageNo {
        self.page_no
    }

    /// Gets a weak reference to the buffer block containing this page.
    ///
    /// This method returns a weak reference to avoid reference cycles between
    /// the buffer page and its containing buffer block.
    ///
    /// # Returns
    /// * `Weak<RefCell<BufBlock>>` - A weak reference to the buffer block
    pub fn buf_page_get_buf_block(&self) -> Weak<RefCell<BufBlock>> {
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
}

/// The buffer control block structure.
///
/// BufBlock represents a buffer block in the buffer pool, which contains both
/// the metadata about a page (BufPage) and a pointer to the actual page data in memory.
/// It also includes a lock for concurrency control.
#[derive(Clone)]
pub struct BufBlock {
    /// Counter that tracks modifications to this buffer block.
    /// Incremented each time the block is removed from the hash table.
    modify_clock: usize,
    /// Metadata about the page stored in this buffer block
    buf_page: RefCell<Rc<RefCell<BufPage>>>,
    /// Pointer to the actual page data in memory
    frame: Frame,
    /// Lock for concurrency control when accessing this buffer block
    pub(crate) rw_lock: Arc<RwLock<()>>,
}

impl BufBlock {
    /// Creates a new buffer block with the specified buffer page and frame.
    ///
    /// # Parameters
    /// * `buf_page` - The buffer page metadata
    /// * `frame` - Pointer to the memory location where the page data is stored
    ///
    /// # Returns
    /// A new BufBlock instance
    pub fn new(buf_page: RefCell<Rc<RefCell<BufPage>>>, frame: Frame) -> Self {
        BufBlock {
            modify_clock: 0,
            buf_page,
            frame,
            rw_lock: Arc::new(Default::default()),
        }
    }

    /// Gets the frame pointer for this buffer block.
    ///
    /// # Returns
    /// A pointer to the memory location where the page data is stored
    pub fn get_frame(&self) -> Frame {
        self.frame
    }

    /// Gets the current state of the buffer page.
    ///
    /// # Returns
    /// The current state of the buffer page
    pub fn buf_block_get_state(&self) -> BufPageState {
        self.buf_page.borrow().borrow().state.clone()
    }

    /// Sets the state of the buffer page.
    ///
    /// This method updates the state of the buffer page, which affects how the page
    /// is managed in the buffer pool (e.g., whether it's in the free list, LRU list, etc.).
    /// The state transition may trigger additional actions such as adding or removing
    /// the page from various buffer pool management lists.
    ///
    /// # Parameters
    /// * `state` - The new state to set for the buffer page
    ///
    /// # Note
    /// This method is currently a placeholder and needs to be implemented.
    pub fn buf_block_set_state(&mut self, state: BufPageState) {
        // TODO: Implement this method to update the page state
    }

    /// Gets the buffer page metadata for this buffer block.
    ///
    /// # Returns
    /// A clone of the buffer page metadata
    pub fn buf_block_get_page(&self) -> Rc<RefCell<BufPage>> {
        self.buf_page.borrow().clone()
    }

    /// Gets a mutable reference to the buffer page metadata for this buffer block.
    ///
    /// This method is similar to `buf_block_get_page` but is intended to be used
    /// when modifications to the buffer page are needed.
    ///
    /// # Returns
    /// * `Rc<RefCell<BufPage>>` - A reference to the buffer page that can be mutated
    pub fn buf_block_get_page_mut(&mut self) -> Rc<RefCell<BufPage>> {
        self.buf_page.borrow().clone()
    }

    /// Increments the modification clock of this buffer block.
    ///
    /// This method is called when the block is removed from the hash table,
    /// typically during page eviction or relocation. The modification clock
    /// helps track how many times the block has been recycled.
    pub fn buf_block_incr_clock(&mut self) {
        self.modify_clock += 1;
    }

    /// Gets a Page object that provides methods for accessing the page data.
    ///
    /// # Returns
    /// A Page object initialized with this buffer block's frame
    pub fn get_page(&self) -> Page {
        Page::new(self.frame)
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

pub struct BufChunk {
    pub mem_size: usize,
    pub size: usize,
    pub mem: *mut u8,
    pub buf_pages: Vec<RefCell<Rc<RefCell<BufBlock>>>>,
}
