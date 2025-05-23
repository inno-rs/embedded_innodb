/** The 2-logarithm of UNIV_PAGE_SIZE: */
pub const UNIV_PAGE_SIZE_SHIFT: usize = 14;

/** The universal page size of the database */
pub const UNIV_PAGE_SIZE: usize = 1 << UNIV_PAGE_SIZE_SHIFT;

/// The following number
/// as the length of a logical field means that the field has SQL nullptr as its value.
pub const UNIV_SQL_NULL: usize = usize::MAX;

pub type Lsn = u64;
pub const IB_FILE_BLOCK_SIZE: usize = 512;

pub type SpaceId = u32;
pub type PageNo = u32;
pub type TxnId = u64;

#[derive(Hash, Clone, Copy, Eq, PartialEq)]
pub struct PageId {
    /// The space id of the page.
    pub space_id: SpaceId,
    /// The page number of the page.
    pub page_no: PageNo,
}

/// InnoDB error codes.
/// Most of the error codes are internal to the engine and will not be seen by user applications.
/// The partial error codes reflect the substate of an operation within InnoDB.
pub enum DbErr {
    /// A successful result.
    DbSuccess,
    /// Internal error cannot proceed.
    DbPanic,

    /* Following are the error codes. */
    /// This is a generic error code.
    /// It is used to classify error conditions that can't be represented by other codes.
    DbError,
    /// Tablespace does not exist or is being deleted right now.
    DbTablespaceDeleted,
}
