use std::{
    fmt::Display,
    sync::{Arc, Mutex, RwLock},
};

use fixedbitset::FixedBitSet;

use crate::{
    buf::buf0types::BufBlock,
    dict::dict0types::{Index, Table},
    innodb::innodb0types::{DbErr, DbErr::DbFail, PageId},
    lock::lock0types::{
        LOCK_COMPATIBILITY_MATRIX, LOCK_GAP, LOCK_INSERT_INTENTION,
        LOCK_TYPE_MASK, LockMode, LockType, REC_LOCK,
    },
    mtr::mtr0mtr::Mtr,
    rem::rem0rec::Record,
    trx::trx0trx::Trx,
    ut::ut0dbg::ut_ad,
};

/// Represents a lock on a database table.
///
/// A table lock is acquired by a transaction to control access to an entire table,
/// preventing conflicting operations from other transactions based on the lock mode.
pub struct TableLock {
    /// Database table in dictionary cache.
    pub table: Arc<Mutex<Table>>,
}

/// Implements the Display trait for TableLock to provide a string representation.
///
/// This implementation displays the table lock with its associated table information.
impl Display for TableLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TableLock: {}", self.table.lock().unwrap())
    }
}

/// Represents a lock on a database record.
///
/// A record lock is acquired by a transaction to control access to a specific record
/// in a table, preventing conflicting operations from other transactions based on the lock mode.
#[derive(Clone)]
pub struct RecLock {
    /// Index of the record.
    pub index: Arc<RwLock<Index>>,
    /// Page Id where the record is located.
    pub page_id: PageId,
}

/// Implements the Display trait for RecLock to provide a string representation.
///
/// This implementation displays the record lock with its page ID information.
impl Display for RecLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RecLock {{ page_id: {} }}", self.page_id)
    }
}

/// Transaction lock type that can be either a table lock or a record lock.
pub enum TxnLock {
    /// Table lock.
    TableLock(TableLock),
    /// Record lock.
    RecLock(RecLock),
}

/// Implements the Display trait for TxnLock to provide a string representation.
///
/// This implementation displays either a table lock or record lock based on the variant,
/// delegating to the respective Display implementations.
impl Display for TxnLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxnLock::TableLock(table_lock) => {
                write!(f, "TableLock: {}", table_lock)
            }
            TxnLock::RecLock(rec_lock) => write!(f, "RecLock: {}", rec_lock),
        }
    }
}

/// Represents a lock in the database system.
///
/// A lock is used to control concurrent access to database resources (tables or records).
/// It contains information about the transaction that owns the lock, the type of lock,
/// the resource being locked, and a bitmap for record locks to track multiple records.
pub struct Lock {
    /// Transaction that owns the lock.
    trx: Arc<Trx>,
    /// Transaction locks associated with this lock (either table or record lock).
    txn_lock: TxnLock,
    /// Bitmap used for record locks to track which records are locked.
    n_bits: FixedBitSet,
    /// Lock mode that determines the type of access granted (e.g., shared, exclusive).
    lock_mode: LockMode,
    /// Type of the lock (e.g., table lock, record lock).
    lock_type: LockType,
}

impl Lock {
    /// Creates a new lock instance.
    ///
    /// # Arguments
    ///
    /// * `trx` - The transaction that owns this lock
    /// * `n_bits` - The capacity of the bitmap for record locks
    /// * `txn_lock` - The type of lock (table or record)
    ///
    /// # Returns
    ///
    /// A new Lock instance with the specified parameters and default lock mode of LOCK_NONE.
    pub fn new(
        trx: Arc<Trx>,
        n_bits: usize,
        txn_lock: TxnLock,
        lock_mode: LockMode,
        lock_type: LockType,
    ) -> Self {
        Lock {
            trx,
            txn_lock,
            n_bits: FixedBitSet::with_capacity(n_bits),
            lock_mode,
            lock_type,
        }
    }

    /// Get the id of the transaction that owns the lock.
    ///
    /// # Returns
    ///
    /// The transaction id of the lock.
    ///
    pub fn trx_id(&self) -> crate::utils::TxnId {
        self.trx.trx_get_id()
    }

    /// Gets the type of the lock.
    ///
    /// # Returns
    ///
    /// The type of the lock as a LockType.
    ///
    pub fn lock_type(&self) -> LockType {
        self.lock_type & LOCK_TYPE_MASK
    }

    /// Get the lock mode of the lock.
    ///
    /// # Returns
    ///
    /// The lock mode of the lock as a LockMode.
    ///
    pub fn lock_mode(&self) -> LockMode {
        self.lock_mode
    }

    /// Check if the gap flag is set for the specified lock.
    ///
    /// This method checks if the lock is a gap lock, which is used to prevent insertions into gaps in the index.
    ///
    /// # Returns
    ///
    /// `true` if the lock is a gap lock, `false` otherwise.
    ///
    pub fn rec_is_gap(&self) -> bool {
        ut_ad!(self.lock_type() == REC_LOCK);

        self.lock_type as u16 & LOCK_GAP != 0
    }

    /// Check if the lock is a record lock that does not block inserts into the gap before the record.
    ///
    /// # Returns
    ///
    /// `true` if the lock is a record lock that does not block inserts into the gap, `false` otherwise.
    ///
    pub fn rec_is_insert_intention(&self) -> bool {
        ut_ad!(self.lock_type() == REC_LOCK);

        self.lock_type as u16 & LOCK_INSERT_INTENTION != 0
    }

    /// Sets the nth bit in the lock bitmap to true.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the bit to set.
    ///
    pub fn rec_set_nth_bit(&mut self, n: usize) {
        ut_ad!(self.lock_type() == REC_LOCK);
        ut_ad!(n < self.n_bits.len());

        self.n_bits.set(n, true);
    }

    /// Gets the total number of bits in the lock bitmap.
    ///
    /// # Returns
    ///
    /// The number of bits in the lock bitmap.
    ///
    pub fn rec_get_n_bits(&self) -> usize {
        ut_ad!(self.lock_type() == REC_LOCK);
        self.n_bits.len()
    }

    /// Find the first set bit in the lock bitmap.
    ///
    /// # Returns
    ///
    /// An `Option<usize>` containing the index of the first set bit, or `None` if no bits are set.
    ///
    pub fn rec_find_set_bit(&self) -> Option<usize> {
        ut_ad!(self.lock_type() == REC_LOCK);
        for i in 0..self.n_bits.len() {
            if self.n_bits[i] {
                return Some(i);
            }
        }

        None
    }

    /// Checks if the nth bit in the lock bitmap is set.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the bit to check.
    ///
    /// # Returns
    ///
    /// `true` if the nth bit is set, `false` otherwise.
    ///
    pub fn rec_is_nth_bit_set(&self, n: usize) -> bool {
        ut_ad!(self.lock_type() == REC_LOCK);
        ut_ad!(n < self.n_bits.len());

        self.n_bits[n]
    }

    /// Resets the nth bit in the lock bitmap to false.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the bit to reset.
    ///
    pub fn rec_reset_nth_bit(&mut self, n: usize) {
        ut_ad!(self.lock_type() == REC_LOCK);
        ut_ad!(n < self.n_bits.len());

        self.n_bits.set(n, false);
    }

    /// Resets all bits in the lock bitmap.
    pub fn rec_bitmap_reset(&mut self) {
        ut_ad!(self.lock_type() == REC_LOCK);
        self.n_bits.clear();
    }

    /// Get the page id of the record lock.
    ///
    /// # Returns
    ///
    /// The page id of the record lock as a PageId.
    ///
    pub fn rec_page_id(&self) -> PageId {
        ut_ad!(self.lock_type() == REC_LOCK);
        match &self.txn_lock {
            TxnLock::RecLock(rec_lock) => rec_lock.page_id,
            _ => panic!("Expected RecLock, found {}", self.txn_lock),
        }
    }

    /// Determines if two lock modes are compatible.
    ///
    /// This method checks if two lock modes can coexist on the same resource without a conflict.
    /// It uses the LOCK_COMPATIBILITY_MATRIX to determine compatibility.
    ///
    /// # Arguments
    ///
    /// * `lhs` - The first lock mode to check
    /// * `rhs` - The second lock mode to check
    ///
    /// # Returns
    ///
    /// `true` if the lock modes are compatible, `false` otherwise.
    pub fn mode_compatible(lhs: LockMode, rhs: LockMode) -> bool {
        LOCK_COMPATIBILITY_MATRIX[lhs as usize][rhs as usize]
    }
}

/// Manages the lock system for transaction concurrency control.
///
/// The LockSys struct provides functionality for managing locks on records and tables,
/// handling lock conflicts, and ensuring transaction isolation levels are maintained.
/// It is a central component in the pessimistic concurrency control mechanism.
pub struct LockSys {}

impl LockSys {
    /// Creates a new instance of the lock management system.
    ///
    /// # Returns
    ///
    /// A new LockSys instance is initialized with default settings.
    pub fn new() -> Self {
        LockSys {}
    }

    /// Checks if locks of other transactions prevent an immediate insertion of a record.
    ///
    /// This method verifies that no conflicting locks exist that would prevent the
    /// current transaction from inserting a record at the specified position.
    /// If conflicting locks are found, the transaction may be suspended and put into
    /// a lock wait state, with a waiting request for a gap exclusive lock added to
    /// the lock queue.
    ///
    /// # Arguments
    ///
    /// * `flags` - Operation flags that control locking behavior
    /// * `rec` - The record being inserted or the record after which insertion occurs
    /// * `buf_block` - The buffer block containing the record
    /// * `index` - The index being modified
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// * `DbSuccess` - If the insertion can proceed immediately
    /// * `DbFail` or other error code - If there are locking conflicts or other issues
    pub fn rec_insert_check_and_lock(
        &self,
        flags: usize,
        rec: Record,
        buf_block: &BufBlock,
        index: &Index,
        mtr: &mut Mtr,
    ) -> DbErr {
        DbFail("".to_string())
    }
}
