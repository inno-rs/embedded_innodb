use crate::os::os0file::OsFile;

/** Start of the data on the page */
pub const FSEG_PAGE_DATA: usize = 38;

pub const FSEG_HEADER_SIZE: usize = 10;

/// Size of page trailer.
pub const FIL_PAGE_DATA_END: usize = 8;

pub type SpaceId = u32;

pub struct FilNode {
    /// Path fo the file.
    name: String,
    /// true if file is open.
    open: bool,
    /// OsFile handle.
    f_handle: Option<OsFile>,
    /// Size of the file in database pages, 0 if unknown.
    size_in_pages: usize,
}

impl FilNode {
    pub fn new() -> FilNode {
        FilNode {
            name: "".to_string(),
            open: false,
            f_handle: None,
            size_in_pages: 0,
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
}
