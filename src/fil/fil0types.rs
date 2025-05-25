pub(crate) use crate::innodb::innodb0types::SpaceId;
use crate::os::os0file::OsFile;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::cell::RefCell;
use std::rc::Rc;

/// Null or undefined page offset in the context of file spaces.
pub const FILL_NULL: u32 = u32::MAX;

/** Start of the data on the page */
pub const FSEG_PAGE_DATA: usize = 38;

pub const FSEG_HEADER_SIZE: usize = 10;

/// Size of page trailer.
pub const FIL_PAGE_DATA_END: usize = 8;

/// Offset of the 4-byte page number in the page header
pub const FIL_PAGE_OFFSET: usize = 4;

/// The offset of the natural predecessor of the page if present.
/// Otherwise, FIL_NULL.
/// This field is not set on BLOB pages which are stored as a singly linked list.
pub const FIL_PAGE_PREV: usize = 8;

/// The offset of the natural successor of the page if present. Otherwise, FIL_NULL.
pub const FIL_PAGE_NEXT: usize = 12;

/// Offset of the 8-byte page LSN in the page header
pub const FIL_PAGE_LSN: usize = 16;

/// SpaceId of the page.
pub const FIL_PAGE_SPACE_ID: usize = 34;

/// Number of bytes reserved at end of page for end-LSN/checksum
pub const FIL_PAGE_END_LSN_CHKSUM: usize = 8;

pub const FIL_SPACE_MAGIC_N: usize = 89472;

pub const FIL_NODE_MAGIC_N: usize = 89389;

pub struct FilNode {
    /// Path fo the file.
    pub name: String,
    /// true if the file is open.
    pub open: bool,
    /// OsFile handle.
    pub f_handle: Option<OsFile>,
    /// Size of the file in database pages, 0 if unknown.
    pub size_in_pages: usize,
    /// Count of the pending i/o on this file. Closing of the file is not allowed if > 0.
    pub n_pending: usize,
    /// Count of pending flushes on the file. Closing of the file is not allowed if > 0.
    pub n_pending_flushes: usize,
    /// Whenever we write to file, we increment this counter.
    pub modification_counter: usize,
    /// Up to what modification_counter, we have flushed the modifications to disk.
    pub flush_counter: usize,
    /// Node for a linked list of files.
    pub chain: LinkedListLink,
    /// LinkedList for LRU.
    pub lru: LinkedListLink,
    /// FIL_NODE_MAGIC_N
    pub magic_n: usize,
}

impl FilNode {
    pub fn new(name: &str, size_in_pages: usize) -> FilNode {
        FilNode {
            name: name.to_string(),
            open: false,
            f_handle: None,
            size_in_pages,
            n_pending: 0,
            n_pending_flushes: 0,
            modification_counter: 0,
            flush_counter: 0,
            chain: Default::default(),
            lru: Default::default(),
            magic_n: FIL_NODE_MAGIC_N,
        }
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn set_file_handle(&mut self, f_handle: OsFile) {
        self.f_handle = Some(f_handle);
    }

    pub fn get_size_in_pages(&self) -> usize {
        self.size_in_pages
    }

    pub fn set_is_open(&mut self) {
        self.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn open(&self) -> bool {
        self.open
    }

    pub fn f_handle(&self) -> &Option<OsFile> {
        &self.f_handle
    }

    pub fn size_in_pages(&self) -> usize {
        self.size_in_pages
    }

    pub fn n_pending(&self) -> usize {
        self.n_pending
    }

    pub fn n_pending_incr(&mut self) {
        self.n_pending += 1;
    }

    pub fn chain(&self) -> &LinkedListLink {
        &self.chain
    }
}

pub enum FilType {
    /// Tablespace
    FIL_TABLESPACE,

    /// Redo Log
    FIL_LOG,
}

impl FilType {
    pub(crate) fn clone(&self) -> FilType {
        match self {
            FilType::FIL_TABLESPACE => FilType::FIL_TABLESPACE,
            FilType::FIL_LOG => FilType::FIL_LOG,
        }
    }
}

/// Container for a FilNode that includes intrusive collection links.
/// This container allows FilNode to be stored in linked lists while maintaining
/// reference counting for safe memory management.
pub struct FilNodeContainer {
    /// Node for a linked list of files.
    pub chain: LinkedListLink,
    /// LinkedList for LRU.
    pub lru: LinkedListLink,
    /// FilNode struct.
    pub fil_node: Rc<RefCell<FilNode>>,
}

intrusive_adapter!(pub FilNodeAdapter = Box<FilNodeContainer>: FilNodeContainer { chain: LinkedListLink });

pub struct FilSpace {
    /// space name = the path to the first file in it.
    pub name: String,

    /// space id.
    pub space_id: SpaceId,

    ///
    pub tablespace_version: i64,

    /// FIL_TABLESPACE or FIL_LOG
    pub fil_type: FilType,

    /// space size in pages; 0 if a single-table namespace whose size we do not know yet. The last incomplete megabytes in data files may be ignored if space = 0.
    pub size_in_pages: usize,

    /// File format, or 0.
    pub flags: usize,

    /// Chain of file nodes.
    pub chain: LinkedList<FilNodeAdapter>,

    /// FIL_SPACE_MAGIC_N
    pub magic_n: usize,
}

/// Container for a FilSpace that includes intrusive collection links.
/// This container allows FilSpace to be stored in linked lists while maintaining
/// reference counting for safe memory management. It also tracks whether the space
/// is in the unflushed spaces list for efficient management of file operations.
pub struct FilSpaceContainer {
    /// List of spaces with at least one unflushed file we have written to.
    pub unflushed_spaces: LinkedListLink,
    /// Is FilSpace present in the unflushed spaces list?
    pub is_in_unflushed_spaces: bool,

    /// List of all spaces.
    pub space_list: LinkedListLink,

    /// FilSpace struct.
    pub fil_space: Rc<RefCell<FilSpace>>,
}
