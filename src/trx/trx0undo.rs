use strum_macros::Display;

/// # Transaction Undo Log Management
///
/// This module implements the transaction undo log functionality for the embedded InnoDB database.
/// Undo logs are used to store information about database modifications that allow the system to:
///
/// - Roll back transactions that are explicitly rolled back by the user
/// - Maintain ACID properties by providing atomicity and durability
/// - Support multi-version concurrency control (MVCC) by maintaining old versions of records
///
/// The undo log is organized as a set of pages that contain undo records. Each undo record
/// contains information about a specific modification (insert, update, or delete) that can
/// be used to reverse the operation if needed.
///
/// There are two types of undo logs:
/// - Insert undo logs: Used for rolling back inserts
/// - Update undo logs: Used for rolling back updates and deletes
///
/// The undo log pages are managed within rollback segments, which are allocated from tablespaces.
use crate::fil::fil0types::{FIL_PAGE_TYPE, FSEG_PAGE_DATA, FilPageType};
use crate::{
    fsp::fsp0fsp::FLST_NODE_SIZE,
    innodb::innodb0types::{PageFrame, PageNo, SpaceId},
    mach::mach0data::{mach_read_from_2, mach_write_to_2},
    mtr::{
        mtr0mtr::{Mtr, MtrLogMode::MTR_LOG_MODE_ALL},
        mtr0types::MlogType::MLOG_UNDO_INIT,
    },
    page::page0page::Page,
    trx::trx0types::UndoRecord,
    utils::TrxId,
};

/// Type of undo log segment
/// Undo entries for insert.
const TRX_UNDO_INSERT: usize = 1;

/// Undo entries for updates and deletion markings.
const TRX_UNDO_UPDATE: usize = 2;

/// States of UndoLogSegment
/// Contains an undo log of an active transaction.
const TRX_UNDO_ACTIVE: usize = 1;

/// Cached for quick reuse.
const TRX_UNDO_CACHED: usize = 2;

/// Insert undo segment can be freed.
const TRX_UNDO_TO_FREE: usize = 3;

/// Update undo segment will not be reused. It can be freed in purge when all undo data in it has been removed.
const TRX_UNDO_TO_PURGE: usize = 4;

/// Contains an undo log segment of a prepared transaction.
const TRX_UNDO_PREPARED: usize = 5;

/// The offset of the undo log on pages of the undo log.
const TRX_UNDO_PAGE_HDR: usize = FSEG_PAGE_DATA;

/// Transaction undo log page headers.
/// TRX_UNDO_INSERT or TRX_UNDO_UPDATE
pub const TRX_UNDO_PAGE_TYPE: usize = 0;

/// Byte offset where the undo log records for the latest transaction start on this page. The first page for undo update log can contain several undo logs.
pub const TRX_UNDO_PAGE_START: usize = 2;

/// Byte offset of the first free byte on each undo log page.
pub const TRX_UNDO_PAGE_FREE: usize = 4;

/// The file list node in the chain of undo log pages.
pub const TRX_UNDO_PAGE_NODE: usize = 6;

/// Size of the Transaction Undo log header size in bytes.
pub const TRX_UNDO_PAGE_HDR_SIZE: usize = 6 + FLST_NODE_SIZE;

/// Undo log header
/// Transaction Id
pub const TRX_UNDO_TRX_ID: usize = 0;

/// Transaction number of the transaction. It is only defined if the log in a history list.
pub const TRX_UNDO_TRX_NO: usize = 8;

/// True if the transaction may have done some delete markings of the records, and thus a purge is necessary. This is only defined in the update log.
pub const TRX_UNDO_DEL_MARKS: usize = 16;

/// Offset of the first undo log record of the header page. Purge operation may remove undo log record from the log start, and therefore this is not necessarily the same as this log header end offset.
pub const TRX_UNDO_LOG_START: usize = 18;

/// True if undo log header includes X/Open XA transaction idenitifier.
pub const TRX_UNDO_XID_EXISTS: usize = 20;

/// Identifier of the table if the preceding field is true.
pub const TRX_UNDO_TABLE_ID: usize = 22;

/// Offset of the next undo log header on the page. 0, if none.
pub const TRX_UNDO_NEXT_LOG: usize = 30;

/// Offset of the prev undo log header on the page. 0, if none.
pub const TRX_UNDO_PREV_LOG: usize = 32;

/// If the log is put to the history list, the file list node is here.
pub const TRX_UNDO_HISTORY_NODE: usize = 34;

/// Represents a page in the undo log.
///
/// An undo page contains undo records that describe how to undo changes made by transactions.
/// The page has a specific structure with headers and data areas for storing undo records.
pub struct UndoPage {
    /// The underlying page in the buffer pool
    page: Page,
}

impl UndoPage {
    /// Creates a new UndoPage instance from a Page.
    ///
    /// # Arguments
    /// * `page` - The underlying page to wrap as an undo page
    ///
    /// # Returns
    /// A new UndoPage instance
    pub fn new(page: Page) -> Self {
        Self { page }
    }

    /// Gets the page number of this undo page.
    ///
    /// # Returns
    /// The page number in the tablespace
    pub fn page_get_page_no(&self) -> PageNo {
        self.page.page_get_fil_offset()
    }

    /// Converts the undo page to a page frame for direct memory access.
    ///
    /// # Returns
    /// A PageFrame representing the raw memory of the page
    pub fn page_to_frame(&self) -> PageFrame {
        self.page.to_page_frame()
    }

    /// Gets the offset of the first free byte on the undo page.
    ///
    /// This is used to determine where new undo records can be written.
    ///
    /// # Returns
    /// The offset of the first free byte on the page
    pub fn page_get_undo_page_free(&self) -> usize {
        let page_frame = self.page.to_page_frame();
        mach_read_from_2(
            &page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_FREE..],
        ) as usize
    }

    /// Initialize the fields in an undo log segment.
    ///
    /// This method sets up a new undo page with the appropriate headers and
    /// initializes it for use as either an insert or update undo log.
    ///
    /// # Arguments
    /// * `undo_type` - The type of undo log (insert or update)
    /// * `mtr` - The mini-transaction that will log this operation
    ///
    /// # Effects
    /// - Sets the undo page type (insert or update)
    /// - Initializes the start and free pointers to point after the header
    /// - Sets the page type to UndoLog
    /// - Logs the initialization in the mini-transaction log
    pub fn trx_undo_page_init(
        &mut self,
        undo_type: TrxUndoType,
        mtr: &mut Mtr,
    ) {
        let page_frame = self.page.to_page_frame();

        // Set the undo page type (insert or update)
        mach_write_to_2(
            &mut page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_TYPE..],
            undo_type.clone() as u16,
        );

        // Initialize the start pointer to point after the header
        mach_write_to_2(
            &mut page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_START..],
            (TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE) as u16,
        );

        // Initialize the free pointer to point after the header
        mach_write_to_2(
            &mut page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_FREE..],
            (TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE) as u16,
        );

        // Set the page type to UndoLog
        mach_write_to_2(
            &mut page_frame.0[FIL_PAGE_TYPE..],
            FilPageType::UndoLog as u16,
        );

        // Log the initialization in the mini-transaction log
        mtr.write_initial_log_record(
            &self.page,
            MLOG_UNDO_INIT,
            MTR_LOG_MODE_ALL,
        );

        // Log the undo type
        mtr.catenate_u64_compressed(undo_type as u64);
    }
}

/// Represents the type of an undo log.
///
/// There are two types of undo logs in InnoDB:
/// - Insert undo logs: Used for rolling back inserts
/// - Update undo logs: Used for rolling back updates and deletes
#[repr(u16)]
#[derive(Display, Clone, Debug)]
pub enum TrxUndoType {
    /// Undo log for insert operations
    UndoInsert = TRX_UNDO_INSERT as u16,
    /// Undo log for update and delete operations
    UndoUpdate = TRX_UNDO_UPDATE as u16,
}

/// Represents the state of an undo log.
///
/// The undo log can be in different states depending on the transaction's
/// progress and the system's management of undo logs.
#[repr(u16)]
#[derive(Display)]
pub enum TrxUndoState {
    /// The undo log is being used by an active transaction
    UndoActive = TRX_UNDO_ACTIVE as u16,
    /// The undo log is cached for quick reuse
    UndoCached = TRX_UNDO_CACHED as u16,
    /// The insert undo log can be freed
    UndoToFree = TRX_UNDO_TO_FREE as u16,
    /// The update undo log will not be reused and can be purged when all data is removed
    UndoToPurge = TRX_UNDO_TO_PURGE as u16,
    /// The undo log belongs to a prepared transaction (in two-phase commit)
    UndoPrepared = TRX_UNDO_PREPARED as u16,
}

/// Represents a transaction undo log.
///
/// An undo log is a sequence of undo records that describe how to undo
/// the changes made by a transaction. Each transaction has its own undo log,
/// which is used for rollback operations and for maintaining MVCC.
pub struct TrxUndo {
    /// Undo log slot number within a rollback segment.
    id: usize,

    /// Transaction undo type (insert or update).
    undo_type: TrxUndoType,

    /// State of the undo log (active, cached, to free, etc.).
    undo_state: TrxUndoState,

    /// Relevant only in the undo update log. This is true if the transaction may have deleted records because of a delete of a row or update of an indexed field. Purge is necessary. This is also true if the transaction has updated an externally stored field.
    del_marks: bool,

    /// Id of the transaction assigned to the undo log.
    trx_id: TrxId,

    /// SpaceId of the undo log.
    space_id: SpaceId,

    /// Page number of the header page in the undo log.
    hdr_page_no: PageNo,
}

impl TrxUndo {
    /// Creates a new transaction undo log with default values.
    ///
    /// # Returns
    /// A new TrxUndo instance initialized with default values
    pub fn new() -> Self {
        Self {
            id: 0,
            undo_type: TrxUndoType::UndoInsert,
            undo_state: TrxUndoState::UndoActive,
            del_marks: false,
            trx_id: 0,
            space_id: 0,
            hdr_page_no: 0,
        }
    }

    /// Get the next record in the undo log.
    ///
    /// This method retrieves the next undo record in the sequence from the current one.
    ///
    /// # Arguments
    /// * `rec` - The current undo record
    /// * `page_no` - The page number where the record is located
    /// * `offset` - The offset within the page
    /// * `mtr` - The mini-transaction for logging
    ///
    /// # Returns
    /// The next undo record in the sequence
    pub fn get_next_rec(
        &self,
        rec: &UndoRecord,
        page_no: PageNo,
        offset: usize,
        mtr: &mut Mtr,
    ) -> UndoRecord {
        UndoRecord {}
    }

    /// Return the end offset of the undo log record of the specified undo log on the page.
    ///
    /// This method determines where an undo record ends on a page, which is necessary
    /// for traversing the undo log.
    ///
    /// # Arguments
    /// * `undo_page` - The undo page containing the record
    /// * `page_no` - The page number to check
    /// * `offset` - The offset of the undo record
    ///
    /// # Returns
    /// The end offset of the undo record on the page
    pub fn trx_undo_page_get_end(
        &self,
        undo_page: &UndoPage,
        page_no: PageNo,
        offset: usize,
    ) -> usize {
        if page_no == undo_page.page_get_page_no() {
            let page_frame = undo_page.page_to_frame();
            let end = mach_read_from_2(&page_frame.0[offset..]);
            if end == 0 {
                // If the end offset is 0, use the free space pointer
                undo_page.page_get_undo_page_free()
            } else {
                end as usize
            }
        } else {
            // If we're checking a different page, use the free space pointer
            undo_page.page_get_undo_page_free()
        }
    }

    /// Get the start offset of the undo log records of the specified undo log on the page.
    ///
    /// This method determines where undo records start on a page, which is necessary
    /// for traversing the undo log.
    ///
    /// # Arguments
    /// * `undo_page` - The undo page to check
    /// * `page_no` - The page number to check
    /// * `offset` - The offset within the page
    ///
    /// # Returns
    /// The start offset of the undo records on the page
    pub fn trx_undo_page_get_start(
        &self,
        undo_page: &UndoPage,
        page_no: PageNo,
        offset: usize,
    ) -> usize {
        if page_no == undo_page.page_get_page_no() {
            // If we're checking the same page, read the start offset from the header
            let page_frame = undo_page.page_to_frame();
            mach_read_from_2(&page_frame.0[offset + TRX_UNDO_LOG_START..])
                as usize
        } else {
            // If we're checking a different page, use the default start position
            TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE
        }
    }
}
