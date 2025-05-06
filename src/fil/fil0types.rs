/** Start of the data on the page */
pub const FSEG_PAGE_DATA: usize = 38;

pub const FSEG_HEADER_SIZE: usize = 10;

/// Size of page trailer.
pub const FIL_PAGE_DATA_END: usize = 8;

pub type SpaceId = u32;

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

    /// space size in pages; 0 if a single-table namespace whose size we do not know yet. Last incomplete megabytes in data files may be ignored if space = 0.
    pub(crate) size_in_pages: usize,

    /// File format, or 0.
    pub flags: usize,
}
