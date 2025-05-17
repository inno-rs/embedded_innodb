use crate::dict::dict0types::{Index, Table};
use crate::innodb::innodb0types::PageId;
use intrusive_collections::{intrusive_adapter, LinkedListLink};
use std::sync::{Arc, Mutex, RwLock};

/// Lock types
pub type LockType = u16;

/// Mask used to extract mode from type_mode field in a lock.
pub const LOCK_MODE_MASK: LockType = 0xF;

/// This flags denotes an ordinary next key lock.
pub const LOCK_ORDINARY: LockType = 0;

/// Table Lock
pub const TABLE_LOCK: LockType = 16;

/// Record Lock
pub const REC_LOCK: LockType = 32;

/// Wait lock flag. This flag is used to indicate that the lock is waiting for its turn in the wait queue.
pub const LOCK_WAIT: LockType = 256;

/// When this bit is set, it means that the lock holds only on the gap before the record. An x-lock on the gap does not give permission to modify the record on which the bit is set. Locks of this type are created when records are removed from the index chain of records.
pub const LOCK_GAP: LockType = 512;

/// This means that the lock is only present on the record and does not block inserts to the gap before the record. This is used in the case when we retrieve a record with a unique key, and is also used in locking plain SELECTs when the user has set READ_COMMITTED isolation level.
pub const LOCK_REC_NOT_GAP: LockType = 1024;

/// This bit is set when we place a waiting gap type record lock request in order to let an insert of an index record to wait until there are no more conflicting locks by other transactions on the gap.
pub const LOCK_INSERT_INTENTION: LockType = 2048;

#[rustfmt::skip]
pub const LOCK_COMPATIBILITY_MATRIX: [[bool; 4]; 4] = [
    /**         IS     IX       S     X   */ //
    /* IS */ [  true,  true,  true,  false], //
    /* IX */ [  true,  true,  false, false], //
    /* S  */ [  true,  false, true,  false], //
    /* X  */ [  false, false, false, false], //
];

/// Basic Lock Mode.
#[derive(Clone, Copy, Debug)]
pub enum LockMode {
    /// Intention shared.
    LOCK_IS = 0,
    /// Intention exclusive.
    LOCK_IX = 1,
    /// Shared.
    LOCK_S = 2,
    /// Exclusive.
    LOCK_X = 3,
    /// No lock.
    LOCK_NONE = 4,
}

pub struct TableLock {
    /// Database table in dictionary cache.
    table: Arc<Mutex<Table>>,
    /// List of locks on the same table.
    locks: LinkedListLink,
}

intrusive_adapter!(TableLockListAdapter = Box<TableLock>: TableLock { locks: LinkedListLink });

pub struct RecLock {
    /// Index of the record.
    index: Arc<RwLock<Index>>,
    /// Page Id.
    page_id: PageId,
    /// List of locks on the same page.
    locks: LinkedListLink,
}

intrusive_adapter!(RecLockListAdapter = Box<RecLock>: RecLock { locks: LinkedListLink });
