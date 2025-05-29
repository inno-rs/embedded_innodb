/************************************************************************
 *                       InnoDB FIL Page Header                         *
 *                       Found at offset 0 of each page                 *
 *                       Total Size: 38 bytes                           *
 ************************************************************************
 * Offset | Size | Field Name         | Description                     *
 *--------+------+--------------------+---------------------------------*
 * 0x00   | 4    | FIL_PAGE_SPACE_ID  | Tablespace ID                   *
 * 0x04   | 4    | FIL_PAGE_OFFSET    | Page number within space        *
 * 0x08   | 4    | FIL_PAGE_PREV      | Prev page in list (or FIL_NULL) *
 * 0x0C   | 4    | FIL_PAGE_NEXT      | Next page in list (or FIL_NULL) *
 * 0x10   | 8    | FIL_PAGE_LSN       | Last modification log sequence   *
 * 0x18   | 2    | FIL_PAGE_TYPE      | Page type (index, undo, etc.)   *
 * 0x1A   | 2    | FIL_PAGE_FILE_FLUSH_LSN | Flush LSN for redo logs    *
 * 0x1C   | 8    | FIL_PAGE_ARCH_LOG_NO_OR_SPACE_ID | Varies by version *
 ************************************************************************/
use crate::fil::fil0types::{
    FilNode, FilNodeContainer, FilSpace, FilSpaceContainer, FilType,
    SpaceId, FIL_SPACE_MAGIC_N,
};
use crate::innodb::innodb0types::{DbErr, PageNo};
use crate::os::os0file::CreateMode::OsFileOpen;
use crate::os::os0file::{os_file_create, IORequest, IOResult, WriteResult};
use crate::ut::ut0dbg::ut_ad;
use intrusive_collections::{
    intrusive_adapter, Adapter, LinkedList, LinkedListLink, UnsafeRef,
};
use log::{error, warn};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

/// Adapter for the unflushed spaces list. This allows FilSpaceContainer to be stored in a linked list
/// using the unflushed_spaces field as the link. UnsafeRef is used because the containers are stored
/// in Arc<RwLock<>> and we need to be able to remove them from the list.
intrusive_adapter!(UnflushedSpaceListAdapter = UnsafeRef<FilSpaceContainer>: FilSpaceContainer { unflushed_spaces: LinkedListLink });

/// Adapter for the file space list. This allows FilSpaceContainer to be stored in a linked list
/// using the space_list field as the link.
intrusive_adapter!(FileSpaceListAdapter = Box<FilSpaceContainer>: FilSpaceContainer { space_list: LinkedListLink });

/// The main file system manager for InnoDB.
/// This struct manages tablespaces, file nodes, and handles I/O operations.
pub struct Fil {
    /// Map of space IDs to FilSpaceContainers. Used for quick lookup of spaces by ID.
    space_by_id: HashMap<SpaceId, Arc<RwLock<FilSpaceContainer>>>,
    /// Map of space names to FilSpaceContainers. Used for quick lookup of spaces by name.
    space_by_name: HashMap<String, Arc<RwLock<FilSpaceContainer>>>,
    /// Current version counter for tablespaces. Incremented each time a new space is created.
    tablespace_version: i64,
    /// Number of files currently open.
    n_open_files: usize,
    /// n_open_files is not allowed to exceed this value.
    max_n_open: usize,
    /// Increment this counter whenever we write to file.
    modification_counter: usize,
    /// Maximum assigned id in the existing tables or assigned during the time the server was up.
    /// We scan the data dictionary during startup and the maximum accordingly.
    max_assigned_id: usize,
    /// List of spaces with at least one unflushed file we have written to.
    unflushed_space_list: LinkedList<UnflushedSpaceListAdapter>,
    /// List of all spaces managed by this Fil instance.
    space_list: LinkedList<FileSpaceListAdapter>,
}

impl Fil {
    /// Creates a new Fil instance with the specified maximum number of open files.
    ///
    /// # Arguments
    ///
    /// * `max_n_open` - The maximum number of files that can be open simultaneously.
    ///
    /// # Returns
    ///
    /// * A new Fil instance.
    ///
    pub fn new(max_n_open: usize) -> Self {
        Fil {
            space_by_id: HashMap::new(),
            space_by_name: HashMap::new(),
            tablespace_version: 0,
            n_open_files: 0,
            max_n_open,
            modification_counter: 0,
            max_assigned_id: 0,
            unflushed_space_list: Default::default(),
            space_list: Default::default(),
        }
    }

    /// Appends a new file to the chain of file space. The File must be closed.
    ///
    /// This method creates a new file node and adds it to the specified tablespace.
    /// It first checks if the space exists, and if it does, creates a new FilNodeContainer
    /// with the specified name and size.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the file to create.
    /// * `size` - The size of the file in database pages.
    /// * `space_id` - The ID of the space to add the file to.
    /// * `is_raw` - Whether the file is a raw device.
    ///
    /// # Returns
    ///
    /// * `true` if the file node was created successfully, `false` if the space does not exist.
    ///
    pub fn node_create(
        &mut self,
        name: &str,
        size: usize,
        space_id: SpaceId,
        is_raw: bool,
    ) -> bool {
        let space = self.space_get_by_id(space_id);
        match space {
            None => false,
            Some(sp) => {
                let fil_space_container = sp.write().unwrap();
                let file_node = Box::new(FilNodeContainer {
                    chain: Default::default(),
                    lru: Default::default(),
                    fil_node: Rc::new(RefCell::new(
                        (FilNode::new(name, size)),
                    )),
                });

                let mut fil_space = fil_space_container.fil_space.borrow_mut();

                fil_space.chain.push_back(file_node);

                ut_ad!(fil_space.chain.iter().count() == 1);

                self.max_assigned_id =
                    if self.max_assigned_id < space_id as usize {
                        space_id as usize
                    } else {
                        self.max_assigned_id
                    };

                true
            }
        }
    }

    /// Creates a space memory object and puts it in the fil_system hash table. If there is an error, prints an error message and returns false.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the space.
    /// * `space_id` - The ID of the space.
    /// * `flags` - The flags for the space.
    /// * `fil_type` - The type of the space (FIL_TABLESPACE or FIL_LOG).
    ///     
    /// # Returns
    ///
    /// * `true` if the space was created successfully, `false` if the space already exists.
    ///
    pub fn space_create(
        &mut self,
        name: String,
        space_id: SpaceId,
        flags: usize,
        fil_type: FilType,
    ) -> bool {
        // Check if the space already exists by name
        if self.space_by_name.contains_key(&name) {
            return false;
        }

        // Check if the space already exists by ID
        if self.space_by_id.contains_key(&space_id) {
            return false;
        }

        self.tablespace_version += 1;

        // Create a new FilSpace instance
        let new_space = Arc::new(RwLock::new(FilSpaceContainer {
            unflushed_spaces: Default::default(),
            is_in_unflushed_spaces: true,
            space_list: Default::default(),
            fil_space: Rc::new(RefCell::new(FilSpace {
                name: name.to_string(),
                space_id,
                tablespace_version: self.tablespace_version,
                fil_type: fil_type.clone(),
                flags,
                size_in_pages: 0,
                chain: Default::default(),
                magic_n: FIL_SPACE_MAGIC_N,
            })),
        }));

        // Insert the new space into both maps
        self.space_by_name.insert(name, new_space.clone());
        self.space_by_id.insert(space_id, new_space.clone());

        {
            let prev_count = self.unflushed_space_list.iter().count();

            let mut b = new_space.write().unwrap();
            let d = b.deref_mut();
            self.unflushed_space_list
                .push_back(unsafe { UnsafeRef::from_raw(d) });

            ut_ad!(self.unflushed_space_list.iter().count() == prev_count + 1);
        }

        true
    }

    /// Retrieves a space by its ID.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to retrieve.
    ///
    /// # Returns
    ///
    /// * `Some(Arc<RwLock<FilSpace>>)` if the space exists, `None` otherwise.
    ///
    pub fn space_get_by_id(
        &self,
        space_id: SpaceId,
    ) -> Option<Arc<RwLock<FilSpaceContainer>>> {
        self.space_by_id.get(&space_id).cloned()
    }

    /// Retrieves a space by its name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the space to retrieve.
    ///
    /// # Returns
    ///
    /// * `Some(Arc<RwLock<FilSpace>>)` if the space exists, `None` otherwise.
    ///
    pub fn space_get_by_name(
        &self,
        name: &str,
    ) -> Option<Arc<RwLock<FilSpaceContainer>>> {
        self.space_by_name.get(name).cloned()
    }

    /// Retrieves the version of a space by its ID.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to retrieve the version for.
    ///
    /// # Returns
    ///
    /// * `Some(i64)` with the tablespace version if the space exists, `None` otherwise.
    ///
    pub fn space_get_version(&self, space_id: SpaceId) -> Option<i64> {
        self.space_get_by_id(space_id).and_then(|space| {
            let space = space.read().unwrap();
            Some(space.fil_space.borrow().tablespace_version)
        })
    }

    /// Retrieves the type of a space by its ID.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to retrieve the type for.
    ///
    /// # Returns
    ///
    /// * `Some(FilType)` with the space type if the space exists, `None` otherwise.
    ///
    pub fn space_get_type(&self, space_id: SpaceId) -> Option<FilType> {
        self.space_get_by_id(space_id).and_then(|space| {
            let space = space.read().unwrap();
            Some(space.fil_space.borrow().fil_type.clone())
        })
    }

    /// Opens a file for a node and updates the file handle in the node.
    ///
    /// # Arguments
    ///
    /// * `file_node` - The file node to open.
    /// * `fil_space` - The space that the file node belongs to.
    /// * `startup` - Whether this is being called during system startup.
    ///
    pub fn node_open_file(
        &mut self,
        file_node: &mut FilNode,
        fil_type: FilType,
        startup: bool,
    ) {
        ut_ad!(file_node.is_open() == false);
        ut_ad!(file_node.n_pending() == 0);

        if file_node.get_size_in_pages() == 0 {
            // Don't know what to do.
        }

        match fil_type {
            FilType::FIL_LOG => {
                let file_handle =
                    os_file_create(file_node.get_name(), OsFileOpen).unwrap();
                file_node.set_file_handle(file_handle);
            }
            FilType::FIL_TABLESPACE => {
                let file_handle =
                    os_file_create(file_node.get_name(), OsFileOpen).unwrap();
                file_node.set_file_handle(file_handle);
            }
        }

        file_node.set_is_open();

        // Open the file and increase the open file count.
        self.n_open_files += 1;
    }

    /// Performs I/O operations on a tablespace.
    ///
    /// # Arguments
    ///
    /// * `io_request` - The type of I/O operation to perform (read or write).
    /// * `batched` - Whether this is a batched I/O operation.
    /// * `space_id` - The ID of the space to perform I/O on.
    /// * `page_no` - The page number within the space.
    /// * `byte_offset` - The byte offset within the page.
    /// * `len` - The length of the data to read or write.
    ///
    /// # Returns
    ///
    /// * `Ok(IOResult)` if the operation was successful, `Err(DbErr)` otherwise.
    ///
    pub fn io(
        &self,
        io_request: IORequest,
        batched: bool,
        space_id: SpaceId,
        page_no: PageNo,
        byte_offset: usize,
        len: usize,
    ) -> Result<IOResult, DbErr> {
        ut_ad!(len > 0);

        let space = self.space_get_by_id(space_id);
        match space {
            None => {
                error!(
                    "Trying to do i/o to a tablespace which does not exist. i/o type: {}, space_id: {}, page_no: {}, i/o length {} bytes. ",
                    io_request, space_id, page_no, len
                );
                return Err(DbErr::DbTablespaceDeleted);
            }
            Some(sp) => {
                let lock_guard = sp.read().unwrap();
            }
        }

        Ok(IOResult::Write(WriteResult {}))
    }

    /// Prepares a file node for I/O. Opens a file if it is closed.
    /// Updates the pending I/O field in the node and the system appropriately.
    /// Takes the node off the LRU list if present.
    ///
    /// # Arguments
    ///
    /// * `node` - The file node to prepare for I/O.
    /// * `space` - The space that the file node belongs to.
    ///
    pub fn node_prepare_for_io(&mut self, space: &mut FilSpace) {
        if self.n_open_files >= self.max_n_open + 5 {
            warn!(
                "Open files {} exceed the limit {}",
                self.n_open_files, self.max_n_open
            );
        }

        let node_container = space.chain.front_mut();
        let mut node = node_container.get().unwrap().fil_node.borrow_mut();

        if node.is_open() == false {
            ut_ad!(node.n_pending() == 0);

            self.node_open_file(
                node.deref_mut(),
                space.fil_type.clone(),
                false,
            );
        }

        node.n_pending_incr();
    }

    /// Frees a tablespace from memory.
    ///
    /// This function removes the space from the unflushed spaces list if it's present there.
    /// It does not actually delete the tablespace files from disk.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to free.
    ///
    /// # Returns
    ///
    /// * `true` if the space was found and freed, `false` if the space was not found.
    ///
    pub fn space_free(&mut self, space_id: SpaceId) -> bool {
        let space = self.space_get_by_id(space_id);
        if space.is_none() {
            return false;
        }

        let binding = space.unwrap();
        let sp = binding.write().unwrap();

        if sp.is_in_unflushed_spaces {
            unsafe {
                let ptr = &*sp;
                let mut cursor =
                    self.unflushed_space_list.cursor_mut_from_ptr(ptr);
                cursor.remove();
            }
        }

        true
    }

    /// Returns the number of spaces in the unflushed spaces list.
    ///
    /// This method counts the number of tablespaces that have at least one unflushed file.
    /// It can be used to monitor the state of the file system and ensure that all
    /// modifications are properly flushed to disk.
    ///
    /// # Returns
    ///
    /// * The number of spaces in the unflushed spaces list.
    ///
    pub fn unflushed_space_list_len(&self) -> usize {
        self.unflushed_space_list.iter().count()
    }

    pub fn tablespace_deleted_or_being_deleted(
        &self,
        space_id: SpaceId,
        table_space_version: i64,
    ) -> bool {
        true
    }
}
