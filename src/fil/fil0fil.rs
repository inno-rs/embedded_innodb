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
use crate::os::os0file::os_file_create;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

struct Fil {
    space_by_id: HashMap<SpaceId, Arc<RwLock<FilSpace>>>,
    space_by_name: HashMap<String, Arc<RwLock<FilSpace>>>,
    tablespace_version: i64,
    /// Number of files currently open.
    n_open_files: usize,
}

impl Fil {
    pub fn new() -> Self {
        Fil {
            space_by_id: HashMap::new(),
            space_by_name: HashMap::new(),
            tablespace_version: 0,
            n_open_files: 0,
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
        }));

        // Insert the new space into both maps
        self.space_by_name.insert(name, new_space.clone());
        self.space_by_id.insert(space_id, new_space);

        true
    }
    pub fn space_get_by_id(
        &self,
        space_id: SpaceId,
    ) -> Option<Arc<RwLock<FilSpace>>> {
        self.space_by_id.get(&space_id).cloned()
    }

    pub fn space_get_by_name(
        &self,
        name: &str,
    ) -> Option<Arc<RwLock<FilSpace>>> {
        self.space_by_name.get(name).cloned()
    }

    pub fn space_get_version(&self, space_id: SpaceId) -> Option<i64> {
        self.space_get_by_id(space_id).and_then(|space| {
            let space = space.read().unwrap();
            Some(space.tablespace_version)
        })
    }

    pub fn space_get_type(&self, space_id: SpaceId) -> Option<FilType> {
        self.space_get_by_id(space_id).and_then(|space| {
            let space = space.read().unwrap();
            Some(space.fil_type.clone())
        })
    }

    pub fn node_open_file(
        &mut self,
        file_node: &mut FilNode,
        fil_space: FilSpace,
        startup: bool,
    ) -> bool {
        if file_node.get_size_in_pages() == 0 {
            todo!()
        }

        match fil_space.fil_type {
            FilType::FIL_LOG => {
                let file_handle = os_file_create(file_node.get_name());
                file_node.set_file_handle(file_handle);
            }
            _ => {
                todo!("Handle other file types");
            }
        }

        file_node.set_is_open();

        // Open the file and increase the open file count.
        self.n_open_files += 1;

        true
    }
}
