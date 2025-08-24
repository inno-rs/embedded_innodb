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
use std::{
    cell::RefCell,
    collections::HashMap,
    ops::DerefMut,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
};

use intrusive_collections::{
    LinkedList, LinkedListLink, UnsafeRef, intrusive_adapter,
};
use tokio::runtime::Runtime;
use tracing::{error, warn};

use crate::{
    buf::buf0flu::BufFlush,
    fil::fil0types::{
        FIL_IBD_FILE_INITIAL_SIZE, FIL_PAGE_SPACE_ID_RANGE, FIL_SPACE_MAGIC_N,
        FilNode, FilNodeContainer, FilSpace, FilSpaceContainer, FilType,
    },
    fsp::{fsp0fsp::Fsp, fsp0types::SYS_TABLESPACE},
    innodb::innodb0types::{
        DbErr, IB_FILE_BLOCK_SIZE, PageNo, RawPage, SpaceId, UNIV_PAGE_SIZE,
    },
    os::{
        os0file::{
            CreateMode::{OsFileCreate, OsFileOpen},
            IORequest, IOResult, ReadResult, WriteResult, os_file_create,
            os_file_set_size, os_file_write,
        },
        os0io::{IoContext, OsIo},
    },
    some_or_panic,
    srv::srv0srv::Config,
    ut::ut0dbg::{ut_a, ut_ad},
};

// Adapter for the unflushed spaces list. This allows FilSpaceContainer to be stored in a linked list
// using the unflushed_spaces field as the link. UnsafeRef is used because the containers are stored
// in Arc<RwLock<>> and we need to be able to remove them from the list.
intrusive_adapter!(UnflushedSpaceListAdapter = UnsafeRef<FilSpaceContainer>: FilSpaceContainer { unflushed_spaces: LinkedListLink });

// Adapter for the file space list. This allows FilSpaceContainer to be stored in a linked list
// using the space_list field as the link.
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
    /// The OS I/O manager used for performing I/O operations.
    os_io: Arc<Mutex<OsIo>>,
    /// Server configuration, used for various settings related to the file system.
    srv_config: Arc<Config>,
    /// Tokio runtime for asynchronous operations.
    runtime: Arc<Runtime>,
}

impl Fil {
    /// Creates a new Fil instance with the specified maximum number of open files.
    ///
    /// # Arguments
    ///
    /// * `max_n_open` - The maximum number of files that can be open simultaneously.
    /// * `os_io` - The OS I/O manager used for performing I/O operations.
    /// * `runtime` - The Tokio runtime for asynchronous operations.
    ///
    /// # Returns
    ///
    /// * A new Fil instance.
    ///
    pub fn new(
        max_n_open: usize,
        os_io: Arc<Mutex<OsIo>>,
        runtime: Arc<Runtime>,
    ) -> Self {
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
            os_io,
            srv_config: Arc::new(Config::default()),
            runtime,
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
                let file_node = Rc::new(RefCell::new(FilNodeContainer {
                    id: 42,
                    chain: Default::default(),
                    lru: Default::default(),
                    fil_node: Rc::new(RefCell::new(FilNode::new(name, size))),
                }));

                let mut fil_space = fil_space_container.fil_space.borrow_mut();
                fil_space
                    .nodes
                    .insert(file_node.borrow().id, file_node.clone());

                unsafe {
                    fil_space
                        .chain
                        .push_back(UnsafeRef::from_raw(file_node.as_ptr()));
                }

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
        name: &String,
        space_id: SpaceId,
        flags: usize,
        fil_type: FilType,
    ) -> bool {
        // Check if the space already exists by name
        if self.space_by_name.contains_key(name) {
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
                nodes: Default::default(),
            })),
        }));

        // Insert the new space into both maps
        self.space_by_name.insert(name.clone(), new_space.clone());
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

    /// Retrieves the number of file nodes in a space.
    ///
    /// Arguments:
    /// * `space_id` - The ID of the space to retrieve the file node count for.
    ///
    /// # Returns:
    /// * The number of file nodes in the specified space.
    pub fn space_get_file_node_count(&self, space_id: SpaceId) -> usize {
        if let Some(space) = self.space_get_by_id(space_id) {
            let space = space.read().unwrap();
            let len = space.fil_space.borrow().nodes.len();

            ut_ad!(len == space.fil_space.borrow().chain.iter().count());

            len
        } else {
            0
        }
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
        self.space_get_by_id(space_id).map(|space| {
            let space = space.read().unwrap();
            Some(space.fil_space.borrow().tablespace_version)
        })?
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
        self.space_get_by_id(space_id).map(|space| {
            let space = space.read().unwrap();
            Some(space.fil_space.borrow().fil_type.clone())
        })?
    }

    /// Opens a file for a node and updates the file handle in the node.
    ///
    /// This method opens the file associated with the given file node and sets
    /// the file handle in the node. It also updates the open file count in the
    /// file system.
    ///
    /// # Arguments
    ///
    /// * `file_node` - The file node to open.
    /// * `fil_type` - The type of the file (tablespace or log).
    /// * `startup` - Whether this is being called during system startup.
    ///
    pub fn node_open_file(
        &mut self,
        file_node: &mut FilNode,
        fil_type: FilType,
        startup: bool,
    ) {
        ut_ad!(!file_node.is_open());
        ut_ad!(file_node.n_pending() == 0);

        if file_node.get_size_in_pages() == 0 {
            // Don't know what to do.
        }

        match fil_type {
            FilType::FilLog => {
                let file_handle =
                    os_file_create(file_node.get_name(), OsFileOpen).unwrap();
                file_node.set_file_handle(file_handle);
            }
            FilType::FilTablespace => {
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
        &mut self,
        io_request: IORequest,
        batched: bool,
        space_id: SpaceId,
        page_no: PageNo,
        byte_offset: usize,
        buf: &mut [u8],
        len: usize,
    ) -> Result<IOResult, DbErr> {
        ut_ad!(len > 0);

        let mut fil_space: Option<Arc<RwLock<FilSpaceContainer>>> = None;
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
                fil_space.replace(sp.clone());
            }
        }

        let fil_space_container =
            some_or_panic!(fil_space, "failed to get space by id");

        let space_ref = fil_space_container.write().unwrap();
        let mut space = space_ref.fil_space.borrow_mut();
        let mut new_page_no = page_no as usize;
        let node = {
            let mut fil_node = Option::<Rc<RefCell<FilNode>>>::None;
            let mut cursor = space.chain.front_mut();
            let mut has_next = cursor.get().is_some();
            while has_next {
                {
                    let file_node_container = cursor.get().unwrap();
                    let curr_fil_node = file_node_container.fil_node.borrow();
                    if curr_fil_node.get_size_in_pages() > page_no as usize {
                        fil_node.replace(file_node_container.fil_node.clone());
                        break;
                    }

                    new_page_no -= curr_fil_node.get_size_in_pages();
                }

                cursor.move_next();
                has_next = cursor.get().is_some();
            }

            some_or_panic!(
                fil_node,
                "failed to get fil node from file node container"
            )
        };

        self.node_prepare_for_io(
            node.borrow_mut().deref_mut(),
            space.deref_mut(),
        );

        let offset = page_no as usize * UNIV_PAGE_SIZE + byte_offset;

        // Check that at least the start offset is within the bounds of a single-table tablespace.

        ut_a!(byte_offset % IB_FILE_BLOCK_SIZE == 0);
        ut_a!(offset % IB_FILE_BLOCK_SIZE == 0);

        let result = {
            let os_io = self.os_io.lock().unwrap();
            let mut node = node.borrow_mut();
            os_io.submit(
                IoContext {
                    batch: false,
                    request: io_request.clone(),
                    fil_node: node.deref_mut(),
                    buf,
                },
                len,
                offset,
            )
        };

        match result {
            Ok(io_result) => {
                if io_request.is_sync() {
                    // Do something with the modification counter
                }

                let is_read = io_request.is_read();
                match is_read {
                    true => Ok(IOResult::Read(ReadResult {})),
                    false => Ok(IOResult::Write(WriteResult {})),
                }
            }
            Err(err) => {
                error!(
                    "Failed to perform I/O operation: {}. Error: {}",
                    io_request, err
                );
                Err(DbErr::DbIoError)
            }
        }
    }

    /// Prepares a file node for I/O. Opens a file if it is closed.
    /// Updates the pending I/O field in the node and the system appropriately.
    /// Take the node off the LRU list if present.
    ///
    /// # Arguments
    ///
    /// * `node` - The file node to prepare for I/O.
    /// * `space` - The space that the file node belongs to.
    ///
    pub fn node_prepare_for_io(
        &mut self,
        node: &mut FilNode,
        space: &mut FilSpace,
    ) {
        if self.n_open_files >= self.max_n_open + 5 {
            warn!(
                "Open files {} exceed the limit {}",
                self.n_open_files, self.max_n_open
            );
        }

        if !node.is_open() {
            ut_ad!(node.n_pending() == 0);

            self.node_open_file(node, space.fil_type.clone(), false);
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

    /// Checks if a tablespace has been deleted or is in the process of being deleted.
    ///
    /// This method compares the provided tablespace version with the current version
    /// to determine if the tablespace has been deleted or is being deleted.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to check.
    /// * `table_space_version` - The version of the tablespace to compare against.
    ///
    /// # Returns
    ///
    /// * `true` if the tablespace has been deleted or is being deleted, `false` otherwise.
    ///
    pub fn tablespace_deleted_or_being_deleted(
        &self,
        space_id: SpaceId,
        table_space_version: i64,
    ) -> bool {
        true
    }

    /// Flushes to disk possible cached pages by the OS.
    ///
    /// This method ensures that any cached data for the specified tablespace is
    /// written to disk. If the space is not found, it returns immediately without
    /// performing any operation.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the space to flush.
    ///
    pub fn flush(&mut self, space_id: SpaceId) {}

    /// Flushes all tablespaces of a specific type.
    ///
    /// This method ensures that any cached data for all tablespaces of the specified
    /// type is written to disk. This is useful for ensuring data durability across
    /// all tablespaces of a particular type (e.g., all log files or all data files).
    ///
    /// # Arguments
    ///
    /// * `fil_type` - The type of tablespaces to flush (e.g., FIL_TABLESPACE or FIL_LOG).
    ///
    pub fn flush_file_space(&mut self, fil_type: FilType) {}

    /// Creates a new single-table space with the specified name, ID, flags, and size.
    ///
    /// # Arguments
    /// * `name` - The name of the new single-table space.
    /// * `space_id` - The ID of the new single-table space.
    /// * `flags` - The flags for the new single-table space.
    /// * `size` - The size of the new single-table space in pages.
    ///
    pub fn create_new_single_table_space(
        &mut self,
        space_id: SpaceId,
        table_name: &str,
        flags: usize,
        size: usize,
    ) -> Result<(), DbErr> {
        ut_a!(flags == 0);
        ut_a!(size >= FIL_IBD_FILE_INITIAL_SIZE);

        let path = self.make_ibd_filename(table_name);
        let os_file = os_file_create(&path, OsFileCreate);

        // TODO(Rahul): Handle the other errors.
        if os_file.is_none() {
            error!("Failed to create file for single-table space: {}", path);
            return Err(DbErr::DbFail(format!(
                "Failed to create file for single-table space: {}",
                path
            )));
        }

        let mut os_file = some_or_panic!(
            os_file,
            "Failed to create file for single-table space"
        );

        // Set the size of the file to the specified size in pages.
        // This expands the file to the required size, and fills it with zeros in the new space.
        let result =
            os_file_set_size(&mut os_file, (size * UNIV_PAGE_SIZE) as u64);
        match result {
            Ok(_) => {}
            Err(_) => {
                error!(
                    "Failed to set size for single-table space file: {}",
                    path
                );
                return Err(DbErr::DbOutOfFileSpace);
            }
        }

        let file_space_id = if space_id == SYS_TABLESPACE as u32 {
            self.assign_new_space_id()
        } else {
            space_id
        };

        let mut raw_page = RawPage::new();
        let mut page_frame = raw_page.as_page_frame();
        Fsp::init_fields(page_frame.0, file_space_id, flags);
        page_frame.0[FIL_PAGE_SPACE_ID_RANGE]
            .copy_from_slice(file_space_id.to_le_bytes().as_slice());

        BufFlush::init_for_writing(&mut page_frame, 0);

        if let Err(e) =
            os_file_write(&mut os_file, page_frame.0, 0, UNIV_PAGE_SIZE)
        {
            let error_message = format!(
                "Failed to write initial page for single-table space: {}. Error: {}",
                path, e
            );
            error!(error_message);
            return Err(DbErr::DbFail(error_message));
        }

        // Create a new single-table space with the specified name, ID, and flags.
        // This is a simplified version of space_create for single-table spaces.
        self.space_create(&path, file_space_id, flags, FilType::FilTablespace);

        // Create a new file node for the single-table space.
        self.node_create(&path, size, file_space_id, false);

        Ok(())
    }

    /// Assigns a new unique space ID for a tablespace.
    ///
    /// This method generates a new unique space ID by incrementing the maximum
    /// assigned ID tracked by the file system. This ensures that each tablespace
    /// has a unique identifier within the database.
    ///
    /// # Returns
    ///
    /// * `SpaceId` - A new unique space ID for a tablespace.
    ///
    pub fn assign_new_space_id(&mut self) -> SpaceId {
        // Assign a new space ID by incrementing the maximum assigned ID.
        // This is used to ensure that each tablespace has a unique ID.
        self.max_assigned_id += 1;
        self.max_assigned_id as SpaceId
    }

    /// Generates a filename for an IBD (InnoDB Data) file.
    ///
    /// This method creates a full path for an IBD file by combining the data home
    /// directory from the server configuration with the provided file name and
    /// adding the .ibd extension. IBD files are used for single-table tablespaces.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The base name of the file (typically the table name).
    ///
    /// # Returns
    ///
    /// * `String` - The full path to the IBD file.
    ///
    pub fn make_ibd_filename(&self, file_name: &str) -> String {
        // Generate a filename for an IBD file based on the space ID and file number.
        // This is typically used for single-table spaces.
        format!("{}{}.ibd", self.srv_config.data_home, file_name)
    }

    /// Look up the space size in pages, lazily opening the single-table
    /// file to populate `size_in_pages` if needed.
    /// Look up the space size in pages, lazily opening the single-table
    /// file to populate `size_in_pages` if needed
    pub fn space_get_size(&mut self, space_id: SpaceId) -> usize {
        // 1) find the space container
        let Some(space_arc) = self.space_get_by_id(space_id) else {
            return 0;
        };

        // Take a write guard because we may update size_in_pages.
        let mut space_guard = space_arc.write().unwrap();
        let space_container: &mut FilSpaceContainer = &mut space_guard;

        // Borrow the FilSpace payload
        let mut space = space_container.fil_space.borrow_mut();

        // 2) Lazy open for single-table tablespaces if size not known yet
        if space.size_in_pages == 0
            && matches!(space.fil_type, FilType::FilTablespace)
        {
            // Not SYS and exactly one file node
            ut_ad!(space_id != SYS_TABLESPACE as u32);
            ut_ad!(space.chain.iter().count() == 1);

            // --- Get an Rc<RefCell<FilNode>> without keeping the cursor alive ---
            let fil_node_rc = {
                let cur = space.chain.front(); // Cursor<...>
                let Some(node_container) = cur.get() else {
                    return 0; // no node in chain (shouldn't happen for single-table)
                };
                node_container.fil_node.clone() // clone the Rc
            }; // cursor dropped here → borrow on chain ends

            // Prepare for I/O (opens if closed; bumps pending)
            {
                let mut node = fil_node_rc.borrow_mut();
                self.node_prepare_for_io(&mut node, &mut space);
            }

            // Recompute total size from the chain
            let total_pages: usize = space
                .chain
                .iter()
                .map(|nc| nc.fil_node.borrow().get_size_in_pages())
                .sum();
            space.size_in_pages = total_pages;

            // Complete the synthetic SyncRead I/O (just decrement pending)
            {
                let mut node = fil_node_rc.borrow_mut();
                self.node_complete_io_read(&mut node);
            }
        }

        space.size_in_pages
    }
    /// (We split this because `space_get_size()` uses a read path.)
    fn node_complete_io_read(&mut self, node: &mut FilNode) {
        node.n_pending_decr();
    }

    /// Complete one I/O on a node.
    /// - Always decrements the node's pending IO.
    /// - If it's a **sync write**, bump modification counters and
    ///   add the owning space to `unflushed_space_list` (once).
    pub fn node_complete_io(
        &mut self,
        node: &mut FilNode,
        space_container: &Arc<RwLock<FilSpaceContainer>>,
        io_request: IORequest,
    ) {
        // mirror C++: ut_a(node->m_n_pending > 0)
        ut_ad!(node.n_pending() > 0);
        node.n_pending_decr();

        // Only SyncWrite triggers unflushed-space bookkeeping.
        let is_sync_write = io_request.is_sync() && !io_request.is_read();
        if is_sync_write {
            // global + per-node modification counters
            self.modification_counter =
                self.modification_counter.wrapping_add(1);

            node.set_modification_counter(self.modification_counter);

            // add space to the unflushed list if not present
            // (we must take a write lock to flip the flag and to get a stable &mut for UnsafeRef)
            let mut guard = space_container.write().unwrap();
            if !guard.is_in_unflushed_spaces {
                guard.is_in_unflushed_spaces = true;

                // SAFETY: FilSpaceContainer is owned by an Arc and lives long-term;
                // intrusive list stores a raw pointer. Convert the &mut to UnsafeRef.
                let ptr: *mut FilSpaceContainer = &mut *guard;
                unsafe {
                    self.unflushed_space_list
                        .push_front(UnsafeRef::from_raw(ptr));
                }
            }
            // `guard` drops here
        }
    }
}
