use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::{Display, Formatter},
    ops::Deref,
    rc::Rc,
    sync::{Arc, Mutex},
};

use fixedbitset::FixedBitSet;
use tracing::info;

use crate::{
    buf::buf0types::BufBlock,
    dict::dict0types::{Index, Table},
    innodb::innodb0types::{DbErr, DbErr::DbFail, PageId, TxnId},
    lock::{
        lock0lock::DeadlockSearchResult::{
            LockExceedMaxDepth, LockVictimIsStart, NoDeadlock,
        },
        lock0types::{
            LOCK_COMPATIBILITY_MATRIX, LOCK_GAP, LOCK_INSERT_INTENTION,
            LOCK_REC, LOCK_REC_NOT_GAP, LOCK_STRENGTH_MATRIX, LOCK_TYPE_MASK,
            LOCK_WAIT, LockId, LockMode,
            LockMode::{LockIX, LockS, LockX},
            LockType,
        },
    },
    mtr::mtr0mtr::Mtr,
    page::page0page::PAGE_HEAP_NO_SUPREMUM,
    rem::rem0rec::Record,
    some_or_panic,
    trx::trx0trx::Trx,
    ut::ut0dbg::ut_ad,
};

/// Restricts the length of the search we do in the wait-for graph of transactions.
const LOCK_MAX_N_STEPS_IN_DEADLOCK_CHECK: usize = 10000;

/// Restricts the recursion depth during deadlock search in the wait-for graph of transactions.
const LOCK_MAX_DEPTH_IN_DEADLOCK_CHECK: usize = 200;

/// Flags for deadlock search.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum DeadlockSearchResult {
    /// The start transaction is the victim.
    LockVictimIsStart,
    /// Some other transaction is victim.
    LockVictimIsOther,
    /// Recursion depth exceeded.
    LockExceedMaxDepth,
    /// There was no deadlock found.
    NoDeadlock,
}

/// Represents a lock on a database table.
///
/// A table lock is acquired by a transaction to control access to an entire table,
/// preventing conflicting operations from other transactions based on the lock mode.
#[derive(Clone)]
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
#[derive(Clone)]
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
    id: LockId,
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
        id: LockId,
        trx: Arc<Trx>,
        n_bits: usize,
        txn_lock: TxnLock,
        lock_mode: LockMode,
        lock_type: LockType,
    ) -> Self {
        Lock {
            id,
            trx,
            txn_lock,
            n_bits: FixedBitSet::with_capacity(n_bits),
            lock_mode,
            lock_type,
        }
    }

    pub fn get_id(&self) -> LockId {
        self.id
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

    /// Check if the lock is in waiting state.
    pub fn is_waiting(&self) -> bool {
        Lock::is_lock_type(self.lock_type, LOCK_WAIT)
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
        ut_ad!(self.lock_type() == LOCK_REC);

        self.lock_type as u16 & LOCK_GAP != 0
    }

    /// Check if the lock is not LOCK_REC_NOT_GAP.
    ///
    /// # Returns
    ///
    /// `true` if the lock is not LOCK_REC_NOT_GAP, `false` otherwise.
    pub fn rec_is_not_gap(&self) -> bool {
        ut_ad!(self.lock_type() == LOCK_REC);

        self.lock_type as u16 & LOCK_REC_NOT_GAP != 0
    }

    /// Check if the lock is a record lock that does not block inserts into the gap before the record.
    ///
    /// # Returns
    ///
    /// `true` if the lock is a record lock that does not block inserts into the gap, `false` otherwise.
    ///
    pub fn rec_is_insert_intention(&self) -> bool {
        ut_ad!(self.lock_type() == LOCK_REC);

        self.lock_type as u16 & LOCK_INSERT_INTENTION != 0
    }

    /// Sets the nth bit in the lock bitmap to true.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the bit to set.
    ///
    pub fn rec_set_nth_bit(&mut self, n: usize) {
        ut_ad!(self.lock_type() == LOCK_REC);
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
        ut_ad!(self.lock_type() == LOCK_REC);
        self.n_bits.len()
    }

    /// Find the first set bit in the lock bitmap.
    ///
    /// # Returns
    ///
    /// An `Option<usize>` containing the index of the first set bit, or `None` if no bits are set.
    ///
    pub fn rec_find_set_bit(&self) -> Option<usize> {
        ut_ad!(self.lock_type() == LOCK_REC);
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
        ut_ad!(self.lock_type() == LOCK_REC);
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
        ut_ad!(self.lock_type() == LOCK_REC);
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
        ut_ad!(self.lock_type() == LOCK_REC);
        self.n_bits.clear();
    }

    /// Get the page id of the record lock.
    ///
    /// # Returns
    ///
    /// The page id of the record lock as a PageId.
    ///
    pub fn rec_page_id(&self) -> PageId {
        ut_ad!(self.lock_type() == LOCK_REC);
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
            lhs == LockMode::LockIS
                || lhs == LockIX
                || lhs == LockS
                || lhs == LockX
        );
        ut_ad!(
            rhs == LockMode::LockIS
                || rhs == LockIX
                || rhs == LockS
                || rhs == LockX
        );

        LOCK_COMPATIBILITY_MATRIX[lhs as usize][rhs as usize]
    }

    /// Determines if one lock mode is stronger than or equal to another lock mode.
    ///
    /// This function uses the LOCK_STRENGTH_MATRIX to determine the strength relationship
    /// between two lock modes. The strength hierarchy is as follows:
    /// - LOCK_X (exclusive) is the strongest lock mode and is stronger than all other modes
    /// - LOCK_S (shared) is stronger than LOCK_IS but not stronger than LOCK_IX or LOCK_X
    /// - LOCK_IX (intention exclusive) is stronger than LOCK_IS but not stronger than LOCK_S or LOCK_X
    /// - LOCK_IS (intention shared) is the weakest and is only stronger than or equal to itself
    ///
    /// # Arguments
    ///
    /// * `lhs` - The left-hand side lock mode to compare
    /// * `rhs` - The right-hand side lock mode to compare against
    ///
    /// # Returns
    ///
    /// `true` if the left-hand side lock mode is stronger than or equal to the right-hand side lock mode,
    /// `false` otherwise.
    pub fn mode_stronger_or_eq(lhs: LockMode, rhs: LockMode) -> bool {
        ut_ad!(
            lhs == LockMode::LockIS
                || lhs == LockIX
                || lhs == LockS
                || lhs == LockX
        );
        ut_ad!(
            rhs == LockMode::LockIS
                || rhs == LockIX
                || rhs == LockS
                || rhs == LockX
        );

        LOCK_STRENGTH_MATRIX[lhs as usize][rhs as usize]
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
    pub fn is_lock_type(lock_type: LockType, target: LockType) -> bool {
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
        ut_ad!(self.lock_type() == LOCK_REC);

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

    /// Determines if the current lock has to wait for another lock.
    ///
    /// This method checks if the current lock is compatible with the given lock.
    /// If the locks are not compatible, it determines whether the current lock
    /// must wait for the other lock to be released before it can be granted.
    ///
    /// For record locks, it performs additional checks using the record's heap number
    /// to determine if specific record locking conditions apply.
    ///
    /// # Arguments
    ///
    /// * `lock` - The other lock to check against
    /// * `heap_no` - The heap number of the record (required for record locks)
    ///
    /// # Returns
    ///
    /// `true` if the current lock has to wait for the given lock, `false` otherwise
    pub fn has_to_wait_for(
        &self,
        lock: &Lock,
        heap_no: Option<usize>,
    ) -> bool {
        ut_ad!(self.lock_type() == LOCK_REC || heap_no.is_none());

        if !Self::mode_compatible(self.lock_mode, lock.lock_mode) {
            if self.lock_type() == LOCK_REC {
                let heap_no = some_or_panic!(heap_no);
                ut_ad!(lock.lock_type() == LOCK_REC);
                ut_ad!(self.rec_is_nth_bit_set(heap_no));
                ut_ad!(lock.rec_is_nth_bit_set(heap_no));

                return lock.rec_blocks(
                    self.trx.clone(),
                    LockModeType {
                        lock_mode: self.lock_mode,
                        lock_type: self.lock_type,
                    },
                    false,
                );
            }

            return true;
        }

        false
    }
}

impl Display for Lock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lock {{ id: {}, trx_id: {} }}",
            self.id,
            self.trx.trx_get_id()
        )
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
    rec_locks: HashMap<PageId, Vec<Rc<RefCell<Lock>>>>,
    /// Map of transaction wait for locks.
    /// This mapping is exactly the same as the one present in wait_lock in Txn.
    /// All txns with que_state == TrxQueLockWait are present here.
    txn_locks: HashMap<TxnId, Rc<RefCell<Lock>>>,
    /// Generate new lock ids by incrementing this counter.
    lock_id_gen: u64,
}

impl LockSys {
    /// Creates a new instance of the lock management system.
    ///
    /// # Returns
    ///
    /// A new LockSys instance is initialized with default settings.
    pub fn new() -> Self {
        LockSys {
            rec_locks: Default::default(),
            txn_locks: Default::default(),
            lock_id_gen: 1,
        }
    }

    fn get_next_lock_id(&mut self) -> LockId {
        let lock_id = self.lock_id_gen;
        self.lock_id_gen += 1;

        LockId(lock_id)
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

    /// Check if a waiting lock request still has to wait in the queue.
    ///
    /// This method determines if a lock that is currently in the waiting state
    /// still needs to wait by checking if there are any other locks on the same
    /// record that would prevent it from being granted.
    ///
    /// # Arguments
    ///
    /// * `waiting_lock` - The lock that is currently waiting
    /// * `heap_no` - The heap number of the record that the lock is waiting for
    ///
    /// # Returns
    ///
    /// `true` if the lock still needs to wait, `false` if it can be granted
    pub fn rec_has_to_wait_in_queue(
        &self,
        waiting_lock: Rc<RefCell<Lock>>,
        heap_no: usize,
    ) -> bool {
        let (page_id, lock_id) = {
            let waiting_lock = waiting_lock.borrow();
            ut_ad!(waiting_lock.is_waiting());
            ut_ad!(Lock::is_lock_type(waiting_lock.lock_type, LOCK_REC));
            ut_ad!(some_or_panic!(waiting_lock.rec_find_set_bit()) == heap_no);

            (waiting_lock.rec_page_id(), waiting_lock.id)
        };

        for lock in self.rec_locks.get(&page_id).unwrap_or(&vec![]) {
            let lock_ref = lock.borrow();
            ut_ad!(Lock::is_lock_type(lock_ref.lock_type, LOCK_REC));
            ut_ad!(lock_ref.rec_page_id() == page_id);

            if lock_ref.id == lock_id {
                // We encountered the same lock. Skip it.
                continue;
            }

            if lock_ref.rec_is_nth_bit_set(heap_no)
                && waiting_lock
                    .borrow()
                    .has_to_wait_for(lock_ref.deref(), Some(heap_no))
            {
                return true;
            }
        }

        false
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
    pub fn rec_create_low(
        &mut self,
        page_id: PageId,
        lock_mode_type: LockModeType,
        heap_no: usize,
        n_bits: usize,
        index: Arc<Index>,
        trx: Arc<Trx>,
    ) -> Rc<RefCell<Lock>> {
        // If rec is the supremum record, then we reset the gap and LOCK_REC_NOT_GAP bits, as all locks on the supremum record are gap locks.
        let lock_mode_type = lock_mode_type;
        let (lock_mode, mut lock_type) =
            (lock_mode_type.lock_mode, lock_mode_type.lock_type);

        if heap_no == PAGE_HEAP_NO_SUPREMUM {
            ut_ad!(lock_type & LOCK_REC_NOT_GAP != 0);

            lock_type &= !(LOCK_GAP | LOCK_REC_NOT_GAP);
        }

        let lock = Rc::new(RefCell::new(Lock::new(
            self.get_next_lock_id(),
            trx.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index, page_id }),
            lock_mode,
            lock_type,
        )));

        lock.borrow_mut().rec_set_nth_bit(heap_no);

        if let Some(locks) = self.rec_locks.get_mut(&page_id) {
            locks.push(lock.clone())
        } else {
            self.rec_locks.insert(page_id, vec![lock.clone()]);
        }

        if Lock::is_lock_type(lock.borrow().lock_type, LOCK_WAIT) {
            self.txn_locks.insert(trx.trx_get_id(), lock.clone());
            trx.set_wait_lock(lock.borrow().id);
        }

        lock
    }

    /// Remove a lock record request from the queue and potentially grant locks to other transactions.
    ///
    /// This method removes a record lock from the lock system's data structures, including:
    /// 1. Removing it from the page's lock list
    /// 2. Removing it from the transaction's lock list
    /// 3. Cleaning up the page entry if no more locks exist for the page
    ///
    /// After removal, it may grant waiting locks that were previously blocked by this lock.
    ///
    /// # Arguments
    ///
    /// * `lock` - The record lock to be removed from the queue
    pub fn rec_deque_from_page(&mut self, lock: Rc<RefCell<Lock>>) {
        // 1. Remove from rec_list.
        let (page_id, lock_id) = {
            let lock_ref = lock.borrow();
            ut_ad!(lock_ref.lock_type() == LOCK_REC);
            (lock_ref.rec_page_id(), lock_ref.id)
        };

        let locks = self.rec_locks.get_mut(&page_id);
        let locks = some_or_panic!(locks);
        let (index, _) = {
            let element_to_delete =
                locks.iter().enumerate().find(|(_index, lock)| {
                    if lock.borrow().rec_page_id() == page_id {
                        true
                    } else {
                        false
                    }
                });

            some_or_panic!(element_to_delete)
        };

        locks.remove(index);

        // 2. Remove from the txn as well.
        lock.borrow().trx.remove_lock(lock_id);

        // 3. Remove the hash entry as well if the list of locks is empty.
        let mut rec_locks_empty = false;
        if locks.is_empty() {
            self.rec_locks.remove(&page_id);
            rec_locks_empty = true;
        }

        // Check if waiting locks in the queue can be granted. If yes, grant them.
        if !rec_locks_empty {}
    }

    pub fn deadlock_occurs(&self, lock: &Lock, trx: Arc<Trx>) -> bool {
        false
    }

    /// Get the next record lock from the list for a specific page and heap number.
    ///
    /// This method iterates through the locks for a given page, starting from the specified index,
    /// and returns the first lock that has the bit set for the given heap number.
    ///
    /// # Arguments
    ///
    /// * `page_id` - The ID of the page containing the record
    /// * `index` - The starting position in the lock list to search from
    /// * `heap_no` - The heap number of the record to find locks for
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - An `Option<Rc<RefCell<Lock>>>` with the next lock if found, or None if no more locks exist
    /// - The next index to use for subsequent searches
    pub fn get_next_rec_lock(
        &self,
        page_id: PageId,
        index: usize,
        heap_no: usize,
    ) -> (Option<Rc<RefCell<Lock>>>, usize) {
        if let Some(locks) = self.rec_locks.get(&page_id) {
            for (i, lock) in locks.iter().enumerate().skip(index) {
                let lock_ref = lock.borrow();
                ut_ad!(lock_ref.lock_type() == LOCK_REC);

                if lock_ref.rec_is_nth_bit_set(heap_no) {
                    return (Some(lock.clone()), i + 1);
                }
            }
        }

        (None, self.rec_locks.len())
    }

    /// Looks recursively for a deadlock in the transaction-lock wait graph.
    ///
    /// This method performs a depth-first search through the wait-for graph to detect
    /// if there is a deadlock involving the given transactions and locks.
    ///
    /// # Arguments
    ///
    /// * `start_trx` - The transaction where the deadlock search started
    /// * `trx` - The current transaction being examined in the recursive search
    /// * `wait_lock` - The lock that the current transaction is waiting for
    /// * `cost` - Counter to limit the total number of steps in the search
    /// * `depth` - Current recursion depth to prevent excessive stack usage
    ///
    /// # Returns
    ///
    /// A `DeadlockSearchResult` indicating whether a deadlock was found and what kind:
    /// - `LockVictimIsStart` if a deadlock is found and the starting transaction should be rolled back
    /// - `LockExceedMaxDepth` if the search exceeded depth or cost limits
    /// - `NoDeadlock` if no deadlock was found
    pub fn deadlock_recursive(
        &self,
        start_trx: Arc<Trx>,
        trx: Arc<Trx>,
        wait_lock: Rc<RefCell<Lock>>,
        cost: &mut usize,
        depth: usize,
    ) -> DeadlockSearchResult {
        info!(
            "start trx={}, trx={}",
            start_trx.trx_get_id(),
            trx.trx_get_id()
        );

        *cost += 1;

        let mut rec_lock_index = 0;

        let mut get_next = || {
            let wait_lock_ref = wait_lock.borrow();

            if wait_lock_ref.lock_type() == LOCK_REC {
                let heap_no = wait_lock_ref.rec_find_set_bit();

                let (lock, index) = self.get_next_rec_lock(
                    wait_lock_ref.rec_page_id(),
                    rec_lock_index,
                    some_or_panic!(heap_no),
                );
                rec_lock_index = index;

                return (lock, heap_no);
            }

            (None, None)
        };

        let wait_lock_lock_type = wait_lock.borrow().lock_type;
        let (mut found_lock, heap_no) =
            if Lock::is_lock_type(wait_lock_lock_type, LOCK_REC) {
                get_next()
            } else {
                (Some(wait_lock.clone()), None)
            };

        let mut found_potential_conflict = found_lock.is_some();
        while found_potential_conflict {
            let lock = some_or_panic!(found_lock.clone());
            let lock_ref = lock.borrow();

            info!(
                "potentially conflicting lock found. wait_for={}, found_lock={}",
                wait_lock.borrow(),
                lock_ref
            );

            if lock_ref.has_to_wait_for(&wait_lock.borrow(), heap_no) {
                let lock_trx = lock_ref.trx.clone();

                if lock_trx.trx_get_id() == start_trx.trx_get_id() {
                    return LockVictimIsStart;
                }

                let too_far = depth > LOCK_MAX_DEPTH_IN_DEADLOCK_CHECK
                    || *cost > LOCK_MAX_N_STEPS_IN_DEADLOCK_CHECK;
                if too_far {
                    // The information about transaction/lock to be rolled back is available in the top level.
                    return LockExceedMaxDepth;
                }

                if let Some(wait_lock) =
                    self.txn_locks.get(&lock_trx.trx_get_id())
                {
                    // recursive call.
                    let ret = self.deadlock_recursive(
                        start_trx.clone(),
                        lock_trx,
                        wait_lock.clone(),
                        cost,
                        depth + 1,
                    );

                    if ret != NoDeadlock {
                        return ret;
                    }
                }
            }

            if heap_no.is_some() {
                (found_lock, _) = get_next();
            }

            found_potential_conflict = found_lock.is_some();
        }

        trx.trx_set_deadlock_mark(false);

        NoDeadlock
    }
}
