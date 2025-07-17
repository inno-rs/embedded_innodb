use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use fixedbitset::FixedBitSet;

use crate::{
    buf::buf0types::BufBlock,
    dict::dict0types::{Index, Table},
    innodb::innodb0types::{DbErr, DbErr::DbFail, PageId},
    lock::lock0types::{
        LOCK_COMPATIBILITY_MATRIX, LOCK_GAP, LOCK_INSERT_INTENTION,
        LOCK_REC_NOT_GAP, LOCK_TYPE_MASK, LockMode,
        LockMode::{LOCK_IX, LOCK_S, LOCK_X},
        LockType, REC_LOCK,
    },
    mtr::mtr0mtr::Mtr,
    page::page0page::PAGE_HEAP_NO_SUPREMUM,
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
    pub index: Arc<Index>,
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

    /// Check if the lock is not LOCK_REC_NOT_GAP.
    ///
    /// # Returns
    ///
    /// `true` if the lock is not LOCK_REC_NOT_GAP, `false` otherwise.
    pub fn rec_is_not_gap(&self) -> bool {
        ut_ad!(self.lock_type() == REC_LOCK);

        self.lock_type as u16 & LOCK_REC_NOT_GAP != 0
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

    /// Resets all bits in the lock bitmap to false.
    ///
    /// This method clears all record locks by setting all bits in the bitmap to false.
    /// It is typically used when releasing all locks held by a transaction on a specific page.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the lock is not a record lock.
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
        ut_ad!(
            lhs == LockMode::LOCK_IS
                || lhs == LOCK_IX
                || lhs == LOCK_S
                || lhs == LOCK_X
        );
        ut_ad!(
            rhs == LockMode::LOCK_IS
                || rhs == LOCK_IX
                || rhs == LOCK_S
                || rhs == LOCK_X
        );

        LOCK_COMPATIBILITY_MATRIX[lhs as usize][rhs as usize]
    }

    /// Checks if a lock type has a specific flag set.
    ///
    /// # Arguments
    ///
    /// * `lock_type` - The lock type to check
    /// * `target` - The flag to check for
    ///
    /// # Returns
    ///
    /// `true` if the lock type has the target flag set, `false` otherwise.
    fn is_lock_type(lock_type: LockType, target: LockType) -> bool {
        (lock_type & target) == target
    }

    /// Determines if this lock blocks a specific transaction's requested lock.
    ///
    /// This method checks if the current lock would prevent another transaction from
    /// acquiring a lock with the specified mode and type. It implements the complex
    /// rules of InnoDB's lock compatibility, including special handling for gap locks,
    /// insert intention locks, and locks on the supremum record.
    ///
    /// # Arguments
    ///
    /// * `trx` - The transaction requesting the lock
    /// * `lock_mode_type` - The mode and type of the requested lock
    /// * `lock_is_on_supremum` - Whether the lock is on the supremum record
    ///
    /// # Returns
    ///
    /// `true` if this lock blocks the requested lock, `false` otherwise.
    pub fn rec_blocks(
        &self,
        trx: Arc<Trx>,
        lock_mode_type: LockModeType,
        lock_is_on_supremum: bool,
    ) -> bool {
        ut_ad!(self.lock_type() == REC_LOCK);

        let lhs_mode = self.lock_mode;
        let LockModeType { lock_mode, lock_type, .. } = lock_mode_type;

        if self.trx_id() != trx.trx_get_id()
            && !Self::mode_compatible(lhs_mode, lock_mode)
        {
            if (lock_is_on_supremum || Self::is_lock_type(lock_type, LOCK_GAP))
                && !Self::is_lock_type(lock_type, LOCK_INSERT_INTENTION)
            {
                // Gap type locks with the LOCK_INSERT_INTENTION flag do not need to wait for anything.
                // This is because different users can have conflicting gap locks.
                return false;
            }

            if !Self::is_lock_type(lock_type, LOCK_INSERT_INTENTION)
                && self.rec_is_gap()
            {
                // Record lock (ORDINARY or LOCK_REC_NOT_GAP does not need to wait for gap type lock.
                return false;
            }

            if Self::is_lock_type(lock_type, LOCK_GAP) && self.rec_is_not_gap()
            {
                // Lock on a gap does not need to wait for LOCK_REC_NOT_GAP type lock.
                return false;
            }

            return !self.rec_is_insert_intention();
        }

        false
    }
}

/// Combines lock mode and lock type information for lock operations.
///
/// This struct is used to pass both the lock mode (e.g., shared, exclusive) and
/// lock type (e.g., gap, record) information together when creating or checking locks.
/// It simplifies method signatures by grouping related lock attributes.
pub struct LockModeType {
    /// The lock mode of the lock.
    pub lock_mode: LockMode,
    /// The type of the lock.
    pub lock_type: LockType,
}

/// Manages the lock system for transaction concurrency control.
///
/// The LockSys struct provides functionality for managing locks on records and tables,
/// handling lock conflicts, and ensuring transaction isolation levels are maintained.
/// It is a central component in the pessimistic concurrency control mechanism.
pub struct LockSys {
    /// Map of record locks, where the key is the PageId and the value is a vector of RecLocks.
    rec_locks: HashMap<PageId, Vec<RecLock>>,
}

impl LockSys {
    /// Creates a new instance of the lock management system.
    ///
    /// # Returns
    ///
    /// A new LockSys instance is initialized with default settings.
    pub fn new() -> Self {
        LockSys { rec_locks: Default::default() }
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

    /// Creates a new record lock with the specified parameters.
    ///
    /// This is a low-level method for creating a record lock. It handles special cases
    /// such as locks on the supremum record, and adds the created lock to the lock system's
    /// internal data structures.
    ///
    /// # Arguments
    ///
    /// * `page_id` - The page ID where the record is located
    /// * `lock_mode_type` - The mode and type of the lock to create
    /// * `heap_no` - The heap number of the record to lock
    /// * `n_bits` - The capacity of the bitmap for record locks
    /// * `index` - The index containing the record
    /// * `trx` - The transaction that will own the lock
    ///
    /// # Returns
    ///
    /// A new Lock instance configured according to the specified parameters.
    fn rec_create_low(
        &mut self,
        page_id: PageId,
        lock_mode_type: LockModeType,
        heap_no: usize,
        n_bits: usize,
        index: Arc<Index>,
        trx: Arc<Trx>,
    ) -> Lock {
        // If rec is the supremum record, then we reset the gap and LOCK_REC_NOT_GAP bits, as all locks on the supremum record are gap locks.
        let lock_mode_type = lock_mode_type;
        let (lock_mode, mut lock_type) =
            (lock_mode_type.lock_mode, lock_mode_type.lock_type);

        if heap_no == PAGE_HEAP_NO_SUPREMUM {
            ut_ad!(lock_type & LOCK_REC_NOT_GAP != 0);

            lock_type &= !(LOCK_GAP | LOCK_REC_NOT_GAP);
        }

        let rec_lock = RecLock { index, page_id };
        let mut lock = Lock::new(
            trx,
            n_bits,
            TxnLock::RecLock(rec_lock.clone()),
            lock_mode,
            lock_type,
        );

        lock.rec_set_nth_bit(heap_no);

        if let Some(locks) = self.rec_locks.get_mut(&page_id) {
            locks.push(rec_lock)
        } else {
            self.rec_locks.insert(page_id, vec![rec_lock]);
        }

        lock
    }
}
