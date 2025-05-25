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
        PageId {
            space_id: 0,
            page_no: 0,
        }
    }
}

/// InnoDB error codes.
/// Most of the error codes are internal to the engine and will not be seen by user applications.
/// The partial error codes reflect the substate of an operation within InnoDB.
#[derive(Eq, PartialEq)]
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
