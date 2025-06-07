/** The 2-logarithm of UNIV_PAGE_SIZE: */
pub const UNIV_PAGE_SIZE_SHIFT: usize = 14;

/** The universal page size of the database */
pub const UNIV_PAGE_SIZE: usize = 1 << UNIV_PAGE_SIZE_SHIFT;

/// The following number
/// as the length of a logical field means that the field has SQL nullptr as its value.
pub const UNIV_SQL_NULL: usize = usize::MAX;

/// Log Sequence Number type, used to track the order of changes in the redo log.
pub type Lsn = u64;

/// The size of a disk block in bytes, used for I/O operations.
/// This is typically the minimum unit of disk I/O on most systems.
pub const IB_FILE_BLOCK_SIZE: usize = 512;

/// Tablespace identifier type.
/// Each tablespace in InnoDB has a unique ID that identifies it in the system.
pub type SpaceId = u32;

/// Page number type within a tablespace.
/// Each page in a tablespace has a unique page number that identifies its position.
pub type PageNo = u32;

/// Transaction identifier type.
/// Each transaction in InnoDB has a unique ID that identifies it in the system.
pub type TxnId = u64;

/// A wrapper around a mutable byte array representing a database page in memory.
///
/// This struct provides a type-safe way to pass page data around the system.
/// The page size is fixed at UNIV_PAGE_SIZE bytes.
pub struct PageFrame<'a>(pub &'a mut [u8; UNIV_PAGE_SIZE]);

#[derive(Hash, Clone, Copy, Eq, PartialEq)]
pub struct PageId {
    /// The space id of the page.
    pub space_id: SpaceId,
    /// The page number of the page.
    pub page_no: PageNo,
}

impl PageId {
    /// Creates a dummy PageId with space_id and page_no both set to 0.
    ///
    /// This method is useful for initializing PageId in test scenarios
    /// or when a placeholder PageId is needed before the actual page
    /// information is available.
    ///
    /// # Returns
    ///
    /// A PageId instance with space_id=0 and page_no=0.
    pub fn dummy() -> Self {
        PageId { space_id: 0, page_no: 0 }
    }
}

/// InnoDB error codes.
/// Most of the error codes are internal to the engine and will not be seen by user applications.
/// The partial error codes reflect the substate of an operation within InnoDB.
#[derive(Eq, PartialEq, Debug)]
pub enum DbErr {
    /// A successful result.
    DbSuccess,
    /// Internal error cannot proceed.
    DbPanic,

    /* Following are the error codes. */
    /// This is a generic error code.
    /// It is used to classify error conditions that can't be represented by other codes.
    DbError,
    /// I/O error occurred.
    DbIoError,
    /// Tablespace does not exist or is being deleted right now.
    DbTablespaceDeleted,
    /// A record in an index would not fit on a compressed page,
    /// or it will become bigger than 1/2 free space in a compressed page frame.
    DbTooBigRecord,
    /// Partial Failure code.
    DbFail,
}

/// Defines the search modes used when positioning a cursor in the B-tree.
///
/// This enum specifies how the cursor should be positioned when the exact search key
/// is not found in the B-tree. Different search modes are used depending on whether
/// the operation needs to find records greater than, less than, or equal to the search key.
pub enum IbSearchMode {
    /// Unsupported
    IbCurrUnSupp = 0,
    /// If the search key is not found,
    /// then position the cursor on the row that is greater than the search key.
    IbCurG = 1,
    /// If the search key is not found,
    /// then position the cursor on the row that is greater than or equal to the search key.
    IbCurGe = 2,
    /// If the search key is not found,
    /// then position the cursor on the row that is less than the search key.
    IbCurL = 3,
    /// If the search key is not found,
    /// then position the cursor on the row that is less than or equal to the search key.
    IbCurLe = 4,
}
