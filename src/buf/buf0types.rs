use crate::fil::fil0types::SpaceId;
use crate::innodb::innodb0types::{Lsn, PageId, PageNo};
use crate::mtr::mtr0mtr::Mtr;
use crate::page::page0page::Page;
use crate::sync::sync0rw::ReadWriteLatch;
use intrusive_collections::{intrusive_adapter, LinkedListLink, UnsafeRef};
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use tokio::sync::RwLock;

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
            freed_page_lock: 0,
            newest_modification: 0,
            oldest_modification: 0,
            free_list: Default::default(),
            flush_list: Default::default(),
            lru_list: Default::default(),
            is_in_free_list: false,
            is_in_flush_list: false,
            is_in_lru_list: false,
            buf_block: Default::default(),
        }
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

    pub fn is_in_free_list(&self) -> bool {
        self.state == BufPageState::BufBlockNotUsed
    }

    pub fn set_is_in_lru_list(&mut self, is_in_lru_list: bool) {
        self.is_in_lru_list = is_in_lru_list;
    }

    pub fn set_is_in_flush_list(&mut self, is_in_flush_list: bool) {
        self.is_in_flush_list = is_in_flush_list;
    }

    pub fn set_is_in_free_list(&mut self, is_in_free_list: bool) {
        self.is_in_free_list = is_in_free_list;
    }

    pub fn is_in_flush_list(&self) -> bool {
        self.is_in_flush_list
    }

    pub fn is_in_lru_list(&self) -> bool {
        self.is_in_lru_list
    }

    /// Check if the block is mapped to a tablespace.
    pub fn in_file(&self) -> bool {
        match self.state.clone() {
            BufPageState::BufBlockFilePage => true,
            _ => false,
        }
    }

    pub fn buf_page_get_buf_block(&self) -> Weak<RefCell<BufBlock>> {
        Weak::clone(&self.buf_block)
    }
}

/// The buffer control block structure.
///
/// BufBlock represents a buffer block in the buffer pool, which contains both
/// the metadata about a page (BufPage) and a pointer to the actual page data in memory.
/// It also includes a lock for concurrency control.
#[derive(Clone)]
pub struct BufBlock {
    /// Metadata about the page stored in this buffer block
    buf_page: RefCell<Rc<RefCell<BufPage>>>,
    /// Pointer to the actual page data in memory
    frame: Frame,
    /// Lock for concurrency control when accessing this buffer block
    pub(crate) lock: Arc<RwLock<()>>,
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
        BufBlock { buf_page, frame, lock: Arc::new(Default::default()) }
    }

    /// Gets the frame pointer for this buffer block.
    ///
    /// # Returns
    /// A pointer to the memory location where the page data is stored
    pub fn getFrame(&self) -> Frame {
        self.frame
    }

    /// Gets the current state of the buffer page.
    ///
    /// # Returns
    /// The current state of the buffer page
    pub fn get_state(&self) -> BufPageState {
        self.buf_page.borrow().borrow().state.clone()
    }

    /// Sets the state of the buffer page.
    ///
    /// # Parameters
    /// * `state` - The new state to set for the buffer page
    pub fn buf_block_set_state(&mut self, state: BufPageState) {}

    /// Gets the buffer page metadata for this buffer block.
    ///
    /// # Returns
    /// A clone of the buffer page metadata
    pub fn buf_block_get_page(&self) -> Rc<RefCell<BufPage>> {
        self.buf_page.borrow().clone()
    }

    pub fn buf_block_get_page_mut(&mut self) -> Rc<RefCell<BufPage>> {
        self.buf_page.borrow().clone()
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
#[derive(Clone, Eq, PartialEq, Debug)]
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
    pub buf_pages: Vec<Pin<Box<BufBlock>>>,
}
