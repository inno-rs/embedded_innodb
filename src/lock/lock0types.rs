use std::fmt::{Display, Formatter};

use strum_macros::Display;

/// Lock types
pub type LockType = u16;

/// Mask used to extract mode from the type_mode field in a lock.
pub const LOCK_MODE_MASK: LockType = 0xF;

/// This flag denotes an ordinary next key lock.
pub const LOCK_ORDINARY: LockType = 0;

/// Table Lock
pub const LOCK_TABLE: LockType = 16;

/// Record Lock
pub const LOCK_REC: LockType = 32;

/// Mask used to extract the type from the type_mode field in a lock.
pub const LOCK_TYPE_MASK: LockType = 0xF0;

/// Wait lock flag. This flag is used to indicate that the lock is waiting for its turn in the wait queue.
pub const LOCK_WAIT: LockType = 256;

/// When this bit is set, it means that the lock holds only on the gap before the record. An x-lock on the gap does not give permission to modify the record on which the bit is set. Locks of this type are created when records are removed from the index chain of records.
pub const LOCK_GAP: LockType = 512;

/// This means that the lock is only present on the record and does not block inserts to the gap before the record. This is used in the case when we retrieve a record with a unique key, and is also used in locking plain SELECTs when the user has set READ_COMMITTED isolation level.
pub const LOCK_REC_NOT_GAP: LockType = 1024;

/// This bit is set when we place a waiting gap type record lock request in order to let an insert of an index record to wait until there are no more conflicting locks by other transactions on the gap.
pub const LOCK_INSERT_INTENTION: LockType = 2048;

/// Lock compatibility matrix.
#[rustfmt::skip]
pub const LOCK_COMPATIBILITY_MATRIX: [[bool; 4]; 4] = [
    /**         IS     IX       S     X     */ //
    /* IS */ [  true,  true,  true,  false  ], //
    /* IX */ [  true,  true,  false, false  ], //
    /* S  */ [  true,  false, true,  false  ], //
    /* X  */ [  false, false, false, false  ], //
];

/// Lock strength matrix. This defines the stronger-or-equal lock relation.
#[rustfmt::skip]
pub const LOCK_STRENGTH_MATRIX: [[bool; 4]; 4] = [
    /**           IS     IX       S     X     */ //
    /* IS */ [  true,  false,  false,  false  ], //
    /* IX */ [  true,  true,   false,  false  ], //
    /* S  */ [  true,  false,  true,   false  ], //
    /* X  */ [  true,  true,   true,    true  ], //
];

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub struct LockId(pub u64);

impl Display for LockId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

/// Basic Lock Mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Display)]
#[repr(u16)]
pub enum LockMode {
    /// Intention shared.
    LockIS = 0,
    /// Intention exclusive.
    LockIX = 1,
    /// Shared.
    LockS = 2,
    /// Exclusive.
    LockX = 3,
    /// No lock.
    LockNone = 4,
}
