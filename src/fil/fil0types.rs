use crate::innodb::innodb0types::{SpaceId, UNIV_PAGE_SIZE};
use crate::os::os0file::OsFile;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// Initial size of the file in pages.
pub const FIL_IBD_FILE_INITIAL_SIZE: usize = 4;
/// Null or undefined page offset in the context of file spaces.
pub const FILL_NULL: u32 = u32::MAX;

/// Checksum of the page.
pub const FIL_PAGE_SPACE_OR_CHKSUM: usize = 0;

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

pub const FIL_PAGE_LSN_RANGE: Range<usize> = (FIL_PAGE_LSN..FIL_PAGE_LSN + 8);
pub const FIL_PAGE_END_LSN_CHKSUM_RANGE: Range<usize> =
    (UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM..UNIV_PAGE_SIZE);
pub const FIL_PAGE_SPACE_OR_CHKSUM_RANGE: Range<usize> =
    (FIL_PAGE_SPACE_OR_CHKSUM..FIL_PAGE_SPACE_OR_CHKSUM + 4);

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
    /// Creates a new file node with the specified name and size.
    ///
    /// # Arguments
    ///
    /// * `name` - The path to the file.
    /// * `size_in_pages` - The size of the file in database pages.
    ///
    /// # Returns
    ///
    /// A new `FilNode` instance with default values and the specified name and size.
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

    /// Returns a reference to the file name.
    ///
    /// # Returns
    ///
    /// A reference to the file name string.
    pub fn get_name(&self) -> &String {
        &self.name
    }

    /// Sets the file handle for this node.
    ///
    /// # Arguments
    ///
    /// * `f_handle` - The OS file handle to set.
    pub fn set_file_handle(&mut self, f_handle: OsFile) {
        self.f_handle = Some(f_handle);
    }

    /// Returns the size of the file in database pages.
    ///
    /// # Returns
    ///
    /// The size of the file in pages.
    pub fn get_size_in_pages(&self) -> usize {
        self.size_in_pages
    }

    /// Marks the file as open.
    ///
    /// This method sets the `open` flag to true, indicating that the file
    /// is currently open and accessible.
    pub fn set_is_open(&mut self) {
        self.open = true;
    }

    /// Checks if the file is currently open.
    ///
    /// # Returns
    ///
    /// `true` if the file is open, `false` otherwise.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns a reference to the file name as a string slice.
    ///
    /// # Returns
    ///
    /// A reference to the file name as a string slice.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Checks if the file is currently open.
    ///
    /// This is an alias for `is_open()`.
    ///
    /// # Returns
    ///
    /// `true` if the file is open, `false` otherwise.
    pub fn open(&self) -> bool {
        self.open
    }

    /// Returns a reference to the file handle.
    ///
    /// # Returns
    ///
    /// A reference to the optional file handle.
    pub fn f_handle(&self) -> &Option<OsFile> {
        &self.f_handle
    }

    /// Returns the size of the file in database pages.
    ///
    /// This is an alias for `get_size_in_pages()`.
    ///
    /// # Returns
    ///
    /// The size of the file in pages.
    pub fn size_in_pages(&self) -> usize {
        self.size_in_pages
    }

    /// Returns the number of pending I/O operations on this file.
    ///
    /// # Returns
    ///
    /// The count of pending I/O operations.
    pub fn n_pending(&self) -> usize {
        self.n_pending
    }

    /// Increments the count of pending I/O operations.
    ///
    /// This method increases the pending I/O counter by 1, indicating
    /// that a new I/O operation has been started on this file.
    pub fn n_pending_incr(&mut self) {
        self.n_pending += 1;
    }

    /// Returns a reference to the chain link for linked list operations.
    ///
    /// # Returns
    ///
    /// A reference to the LinkedListLink used for chaining file nodes.
    pub fn chain(&self) -> &LinkedListLink {
        &self.chain
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilType {
    /// Tablespace
    FilTablespace,

    /// Redo Log
    FilLog,
}

impl FilType {
    /// Creates a clone of this FilType.
    ///
    /// This method provides an explicit implementation of the clone operation
    /// for FilType values, returning a new instance with the same variant.
    ///
    /// # Returns
    ///
    /// A new FilType instance with the same variant as this one.
    pub(crate) fn clone(&self) -> FilType {
        match self {
            FilType::FilTablespace => FilType::FilTablespace,
            FilType::FilLog => FilType::FilLog,
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

/// Represents a tablespace in the InnoDB file system.
///
/// A `FilSpace` manages a collection of files that make up a single logical tablespace.
/// It tracks metadata about the tablespace such as its name, ID, version, and size,
/// as well as maintaining a chain of file nodes that represent the physical files.
pub struct FilSpace {
    /// Space name, which is the path to the first file in the tablespace.
    pub name: String,

    /// Unique identifier for this tablespace.
    pub space_id: SpaceId,

    /// Version number of the tablespace, incremented on certain operations.
    pub tablespace_version: i64,

    /// Type of the tablespace (FIL_TABLESPACE or FIL_LOG).
    pub fil_type: FilType,

    /// Total size of the tablespace in pages.
    /// A value of 0 indicates a single-table namespace whose size is not yet known.
    /// The last incomplete megabytes in data files may be ignored if space = 0.
    pub size_in_pages: usize,

    /// File format flags, or 0 if not specified.
    pub flags: usize,

    /// Linked list of file nodes that make up this tablespace.
    pub chain: LinkedList<FilNodeAdapter>,

    /// Magic number used to verify the integrity of the structure.
    /// Should always be set to FIL_SPACE_MAGIC_N.
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
