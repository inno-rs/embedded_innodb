/*
╔═══════════════════════════════╦═══════╦══════════════════════════════════════════════╗
║ Name                          ║ Value ║ Meaning                                      ║
╠═══════════════════════════════╬═══════╬══════════════════════════════════════════════╣
║ MLOG_UNKNOWN                  ║   0   ║ Unknown log type                             ║
║ MLOG_1BYTE                    ║   1   ║ Write 1 byte                                 ║
║ MLOG_2BYTES                   ║   2   ║ Write 2 bytes                                ║
║ MLOG_4BYTES                   ║   4   ║ Write 4 bytes                                ║
║ MLOG_8BYTES                   ║   8   ║ Write 8 bytes                                ║
║ MLOG_REC_INSERT               ║   9   ║ Insert a record                              ║
║ MLOG_REC_CLUST_DELETE_MARK    ║  10   ║ Mark a clustered-index record deleted        ║
║ MLOG_REC_SEC_DELETE_MARK      ║  11   ║ Mark a secondary-index record deleted        ║
║ MLOG_REC_UPDATE_IN_PLACE      ║  13   ║ Update record in place (same field sizes)     ║
║ MLOG_REC_DELETE               ║  14   ║ Physically delete a record from a page       ║
║ MLOG_LIST_END_DELETE          ║  15   ║ End of delete-list on an index page          ║
║ MLOG_LIST_START_DELETE        ║  16   ║ Start of delete-list on an index page        ║
║ MLOG_LIST_END_COPY_CREATED    ║  17   ║ Copy end of delete list to a new page        ║
║ MLOG_PAGE_REORGANIZE          ║  18   ║ Reorganize (defragment) an index page        ║
║ MLOG_PAGE_CREATE              ║  19   ║ Create a new index page                      ║
║ MLOG_UNDO_INSERT              ║  20   ║ Insert into an undo-log page                 ║
║ MLOG_UNDO_ERASE_END           ║  21   ║ Erase end of an undo-log page                ║
║ MLOG_UNDO_INIT                ║  22   ║ Initialize an undo-log page                  ║
║ MLOG_UNDO_HDR_DISCARD         ║  23   ║ Discard an undo-log header                   ║
║ MLOG_UNDO_HDR_REUSE           ║  24   ║ Reuse an undo-log header                     ║
║ MLOG_UNDO_HDR_CREATE          ║  25   ║ Create an undo-log header                    ║
║ MLOG_REC_MIN_MARK             ║  26   ║ Mark an index record as “minimum”            ║
║ MLOG_FULL_PAGE                ║  28   ║ Write the full contents of a page            ║
║ MLOG_INIT_FILE_PAGE           ║  29   ║ Initialize a file page (ignore prior data)    ║
║ MLOG_WRITE_STRING             ║  30   ║ Write an arbitrary byte-string to a page     ║
║ MLOG_MULTI_REC_END            ║  31   ║ End of a multi-record write sequence         ║
║ MLOG_DUMMY_RECORD             ║  32   ║ Pad out a log block (no-op)                  ║
║ MLOG_FILE_CREATE              ║  33   ║ .ibd file creation                            ║
║ MLOG_FILE_RENAME              ║  34   ║ .ibd file rename                              ║
║ MLOG_FILE_DELETE              ║  35   ║ .ibd file deletion                            ║
║ MLOG_BIGGEST_TYPE             ║  47   ║ Highest valid log record type (assert check) ║
║ MLOG_SINGLE_REC_FLAG (flag)    ║ 128   ║ OR-flag: single-record MTR optimization       ║
╚═══════════════════════════════╩═══════╩══════════════════════════════════════════════╝
*/
use strum_macros::Display;

#[derive(Display)]
pub enum MtrLogMode {
    /// Default log mode. Log all operations modifying the disk based data.
    MTR_LOG_MODE_ALL = 0,
    /// Log only operations modifying file space page allocation data(e.g., fsp0fsp.rs).
    MTR_LOG_SPACE = 1,
    /// Do not log any operations.
    MTR_LOG_NONE = 2,
    /// Inserts are logged in short form.
    MTR_LOG_SHORT_INSERTS = 3,
}

#[derive(Display)]
pub enum MlogType {
    /// Unknown log type.
    MLOG_UNKNOWN = 0,
    /// 1 byte is written.
    MLOG_1BYTE = 1,
    /// 2 bytes are written.
    MLOG_2BYTES = 2,
    /// 4 bytes are written.
    MLOG_4BYTES = 4,
    /// 8 bytes are written.
    MLOG_8BYTES = 8,
    /// Record is inserted.
    MLOG_REC_INSERT = 9,
    /// Mark a clustered-index record deleted.
    MLOG_REC_CLUST_DELETE_MARK = 10,
    /// Mark a secondary-index record deleted.
    MLOG_REC_SEC_DELETE_MARK = 11,
    /// Update record in place (same field sizes).
    MLOG_REC_UPDATE_IN_PLACE = 13,
    /// Delete a record from a page.
    MLOG_REC_DELETE = 14,
    /// Delete record list end on index page.
    MLOG_LIST_END_DELETE = 15,
    /// Delete record list start on index page.
    MLOG_LIST_START_DELETE = 16,
    /// Copy the record list end to a newly created index page.
    MLOG_LIST_END_COPY_CREATED = 17,
    /// Reorganize (defragment) an index page.
    MLOG_PAGE_REORGANIZE = 18,
    /// Create a new index page.
    MLOG_PAGE_CREATE = 19,
    /// Insert into an undo-log page.
    MLOG_UNDO_INSERT = 20,
    /// Erase end of an undo-log page.
    MLOG_UNDO_ERASE_END = 21,
    /// Initialize an undo-log page.  
    MLOG_UNDO_INIT = 22,
    /// Discard an undo-log header.
    MLOG_UNDO_HDR_DISCARD = 23,
}

#[derive(Display)]
pub enum MtrState {
    /// Undefined state.
    MTR_UNDEFINED = 0,
    /// The mini-transaction is active.
    MTR_ACTIVE = 123,
    /// The mini-transaction is in the process of committing.
    MTR_COMMITTING = 64,
    /// The mini-transaction has committed.
    MTR_COMMITTED = 76,
}
