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
use crate::fil::fil0types::{FilNode, FilSpace, FilType, SpaceId};
use crate::innodb::innodb0types::{DbErr, PageNo};
use crate::os::os0file::CreateMode::OsFileOpen;
use crate::os::os0file::{os_file_create, IORequest, IOResult, WriteResult};
use crate::ut::ut0dbg::ut_ad;
use log::{error, warn};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct Fil {
    space_by_id: HashMap<SpaceId, Arc<RwLock<FilSpace>>>,
    space_by_name: HashMap<String, Arc<RwLock<FilSpace>>>,
    tablespace_version: i64,
    /// Number of files currently open.
    n_open_files: usize,
    /// n_open_files is not allowed to exceed this value.
    max_n_open: usize,
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
        }
    }

    /// Appends a new file to the chain of file space. The File must be closed.
    pub fn node_create(
        &self,
        name: &str,
        size: usize,
        space_id: SpaceId,
        is_raw: bool,
    ) {
        let file_node = Box::new(FilNode::new(name, size));

        let space = self.space_get_by_id(space_id).unwrap();
        {
            let mut fil_space = space.write().unwrap();
            fil_space.chain.push_back(file_node);

            ut_ad!(fil_space.chain.iter().count() == 1);
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
        let new_space = Arc::new(RwLock::new(FilSpace {
            name: name.to_string(),
            space_id,
            tablespace_version: self.tablespace_version,
            fil_type: fil_type.clone(),
            flags,
            size_in_pages: 0,
            chain: Default::default(),
        }));

        // Insert the new space into both maps
        self.space_by_name.insert(name, new_space.clone());
        self.space_by_id.insert(space_id, new_space);

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
    ) -> Option<Arc<RwLock<FilSpace>>> {
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
    ) -> Option<Arc<RwLock<FilSpace>>> {
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
            Some(space.tablespace_version)
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
            Some(space.fil_type.clone())
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
        fil_space: &mut FilSpace,
        startup: bool,
    ) {
        ut_ad!(file_node.is_open() == false);
        ut_ad!(file_node.n_pending() == 0);

        if file_node.get_size_in_pages() == 0 {
            // Don't know what to do.
        }

        match fil_space.fil_type {
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

        if node.is_open() == false {
            ut_ad!(node.n_pending() == 0);

            self.node_open_file(node, space, false);
        }

        node.n_pending_incr();
    }
}
