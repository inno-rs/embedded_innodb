use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::{Display, Formatter},
    ops::Deref,
    rc::Rc,
    sync::{Arc, RwLock},
};

use fixedbitset::FixedBitSet;
use tracing::info;

use crate::{
    buf::buf0types::BufBlock,
    dict::{
        dict0table::{Table, TableId},
        dict0types::Index,
    },
    innodb::innodb0types::{DbErr, PageId, TrxId},
    lock::{
        lock0lock::DeadlockSearchResult::{
            LockExceedMaxDepth, LockVictimIsStart, NoDeadlock,
        },
        lock0types::{
            LOCK_COMPATIBILITY_MATRIX, LOCK_GAP, LOCK_INSERT_INTENTION,
            LOCK_REC, LOCK_REC_NOT_GAP, LOCK_STRENGTH_MATRIX, LOCK_TABLE,
            LOCK_TYPE_MASK, LOCK_WAIT, LockId, LockMode,
            LockMode::{LockIX, LockS, LockX},
            LockType,
        },
    },
    mtr::mtr0mtr::Mtr,
    page::{page0cur::PageCur, page0page::PAGE_HEAP_NO_SUPREMUM},
    rem::rem0rec::Record,
    some_or_panic,
    trx::{trx0trx::Trx, trx0types::TrxQueState::TrxQueLockWait},
    ut::ut0dbg::ut_ad,
    ut_panic,
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
    pub table: Arc<Table>,
}

/// Implements the Display trait for TableLock to provide a string representation.
///
/// This implementation displays the table lock with its associated table information.
impl Display for TableLock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TableLock: {}", self.table)
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

    /// Returns the unique identifier of the lock.
    ///
    /// # Returns
    ///
    /// The lock ID as a LockId.
    ///
    pub fn get_id(&self) -> LockId {
        self.id
    }

    /// Get the id of the transaction that owns the lock.
    ///
    /// # Returns
    ///
    /// The transaction id of the lock.
    ///
    pub fn trx_id(&self) -> TrxId {
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

    /// Check if the lock is in the waiting state.
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
        ut_ad!(Lock::is_lock_type(self.lock_type, LOCK_REC));
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
        ut_ad!(Lock::is_lock_type(self.lock_type, LOCK_REC));
        ut_ad!(n < self.n_bits.len());

        self.n_bits.set(n, false);
    }

    /// Resets a waiting lock by removing the LOCK_WAIT flag.
    ///
    /// This method transitions a lock from the waiting state to the granted state
    /// by clearing the LOCK_WAIT flag and removing the wait lock reference from
    /// the transaction.
    ///
    pub fn reset(&mut self) {
        ut_ad!(self.is_waiting());
        ut_ad!(self.id == some_or_panic!(self.trx.get_wait_lock()));

        self.lock_type &= !LOCK_WAIT;
        self.trx.set_wait_lock(None);
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
            self.trx.trx_get_id().0
        )
    }
}

/// Combines lock mode and lock type information for lock operations.
///
/// This struct is used to pass both the lock mode (e.g., shared, exclusive) and
/// lock type (e.g., gap, record) information together when creating or checking locks.
/// It simplifies method signatures by grouping related lock attributes.
#[derive(Clone)]
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
    /// Map of table locks, where the key is the TableId and the value is a vector of TableLocks.
    table_locks: HashMap<TableId, Vec<Rc<RefCell<Lock>>>>,
    /// Map of transaction wait for locks.
    /// This mapping is exactly the same as the one present in wait_lock in Txn.
    /// All txns with que_state == TrxQueLockWait are present here.
    txn_locks: HashMap<TrxId, Rc<RefCell<Lock>>>,
    /// Generate new lock ids by incrementing this counter.
    lock_id_gen: u64,
}

struct MoveLockRequest {
    lock_mode_type: LockModeType,
    heap_no: usize,
    trx: Arc<Trx>,
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
            table_locks: Default::default(),
            txn_locks: Default::default(),
            lock_id_gen: 1,
        }
    }

    /// Generates and returns the next available lock ID.
    ///
    /// This method increments the internal lock ID counter to ensure
    /// each lock has a unique identifier within the lock system.
    ///
    /// # Returns
    ///
    /// A new unique LockId.
    ///
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
    /// * `()` - If the insertion can proceed immediately
    /// * `DbFail` or other error code - If there are locking conflicts or other issues
    pub fn rec_insert_check_and_lock(
        &mut self,
        flags: usize,
        rec: Record,
        buf_block: Arc<RwLock<BufBlock>>,
        index: Arc<Index>,
        trx: Arc<Trx>,
        mtr: &mut Mtr,
    ) -> Result<(), DbErr> {
        let next_rec = some_or_panic!(rec.rec_get_next());
        let next_rec_heap_no = next_rec.rec_get_heap_no();
        // When inserting a record in the table, the table must have at least IX lock.
        ut_ad!(
            self.table_has(
                trx.clone(),
                index.get_table(),
                LockModeType { lock_mode: LockIX, lock_type: 0 }
            )
            .is_some()
        );

        let page_id = buf_block.read().unwrap().get_page_id();
        let page = buf_block.read().unwrap().get_page();

        if self.rec_locks.contains_key(&page_id) == false {
            if index.is_clustered() == false {
                page.page_update_max_trx_id(trx.trx_get_id(), mtr);
            }

            return Ok(());
        }

        let lock_mode_type = LockModeType {
            lock_mode: LockX,
            lock_type: LOCK_GAP | LOCK_INSERT_INTENTION,
        };

        let ret = if let Some(wait_for) = self.rec_other_has_conflicting(
            page_id,
            lock_mode_type.clone(),
            next_rec_heap_no,
            trx.clone(),
        ) {
            self.rec_enqueue_waiting(
                lock_mode_type,
                buf_block,
                next_rec_heap_no,
                index.clone(),
                trx.clone(),
            )
        } else {
            Ok(())
        };

        match &ret {
            Ok(_) if index.is_clustered() == false => {
                page.page_update_max_trx_id(trx.trx_get_id(), mtr);
            }
            _ => {}
        }

        ret
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
    /// A new Lock instance is configured according to the specified parameters.
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

        lock_type |= LOCK_REC;

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

        trx.add_lock(lock.borrow().id);

        if Lock::is_lock_type(lock.borrow().lock_type, LOCK_WAIT) {
            self.txn_locks.insert(trx.trx_get_id(), lock.clone());
            trx.set_wait_lock(Some(lock.borrow().id));
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
    ///
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

    /// Checks if a deadlock occurs when a transaction tries to acquire a lock.
    ///
    /// This function performs deadlock detection by recursively traversing the wait-for graph
    /// of transactions and locks.
    /// If a deadlock is detected, the function will determine which
    /// transaction should be rolled back to resolve the deadlock.
    ///
    /// # Arguments
    ///
    /// * `lock` - The lock that the transaction is trying to acquire
    /// * `trx` - The transaction that is trying to acquire the lock
    ///
    /// # Returns
    ///
    /// * `true` if a deadlock is detected and the current transaction should be rolled back
    /// * `false` if no deadlock is detected
    pub fn deadlock_occurs(
        &self,
        lock: Rc<RefCell<Lock>>,
        trx: Arc<Trx>,
    ) -> bool {
        loop {
            let result = self.deadlock_recursive(
                trx.clone(),
                trx.clone(),
                lock.clone(),
                &mut 0,
                0,
            );

            match result {
                LockVictimIsStart => {
                    info!("*** WE ROLL BACK TRANSACTION (2)");
                    break;
                }
                DeadlockSearchResult::LockVictimIsOther => {
                    // We chose some other transaction for rollback. Continue checking.
                }
                LockExceedMaxDepth => {
                    info!(
                        "TOO DEEP OR LONG SEARCH IN THE LOCK TABLE WAITS-FOR GRAPH, WE WILL ROLL BACK FOLLOWING TRANSACTION"
                    );
                    info!("\n*** TRANSACTION:\n");
                    // Print transaction and lock information.
                    break;
                }
                NoDeadlock => {
                    return false;
                }
            }
        }

        true
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
    /// - The next index to use for later searches
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

    /// Checks if some other transaction has a conflicting explicit lock request in the queue.
    ///
    /// This function examines the record locks on a specific page to determine if any transaction
    /// other than the one provided has a lock that conflicts with the specified lock mode and type.
    /// It is used during lock acquisition to detect potential conflicts that would require waiting.
    ///
    /// # Arguments
    ///
    /// * `page_id` - The page ID where the record is located
    /// * `lock_mode_type` - The lock mode and type to check for conflicts against
    /// * `heap_no` - The heap number identifying the specific record
    /// * `trx` - The transaction that is attempting to acquire a lock
    ///
    /// # Returns
    ///
    /// An `Option<Rc<RefCell<Lock>>>` containing a reference to the conflicting lock if one exists,
    /// or `None` if no conflicting locks are found.
    ///
    pub fn rec_other_has_conflicting(
        &self,
        page_id: PageId,
        lock_mode_type: LockModeType,
        heap_no: usize,
        trx: Arc<Trx>,
    ) -> Option<Rc<RefCell<Lock>>> {
        if let Some(locks) = self.rec_locks.get(&page_id) {
            for lock in locks {
                let lock_ref = lock.borrow();
                ut_ad!(lock_ref.lock_type() == LOCK_REC);

                if lock_ref.rec_is_nth_bit_set(heap_no)
                    && lock_ref.rec_blocks(
                        trx.clone(),
                        lock_mode_type.clone(),
                        heap_no == PAGE_HEAP_NO_SUPREMUM,
                    )
                {
                    return Some(lock.clone());
                }
            }
        }

        None
    }

    /// Checks if a transaction has a GRANTED explicit lock on a record that is stronger or equal to the specified mode.
    ///
    pub fn rec_has_expl(
        &self,
        page_id: PageId,
        lock_mode_type: LockModeType,
        heap_no: usize,
        trx: Arc<Trx>,
    ) -> Option<Rc<RefCell<Lock>>> {
        ut_ad!(
            lock_mode_type.lock_mode == LockX
                || lock_mode_type.lock_mode == LockS
        );
        ut_ad!(!Lock::is_lock_type(
            lock_mode_type.lock_type,
            LOCK_INSERT_INTENTION
        ));

        None
    }

    /// Adds a record lock to the queue for a specific buffer block, heap number, and index.
    ///
    /// This function is used to add a new lock to a record or to transfer locks during operations
    /// like page splits or merges. It handles special cases like supremum records and ensures
    /// proper lock type flags are set.
    ///
    /// # Arguments
    ///
    /// * `lock_mode_type` - The mode and type of the lock to be added
    /// * `buf_block` - The buffer block containing the record to be locked
    /// * `heap_no` - The heap number of the record to be locked
    /// * `index` - The index to which the record belongs
    /// * `trx` - The transaction that is requesting the lock
    pub fn rec_add_to_queue(
        &mut self,
        lock_mode_type: LockModeType,
        buf_block: Arc<RwLock<BufBlock>>,
        heap_no: usize,
        index: Arc<Index>,
        trx: Arc<Trx>,
    ) {
        let mut lock_mode_type = lock_mode_type;
        // If rec is the supremum record, then we can reset the gap bit as all the locks on the supremum are automatically of the gap type, and we try to avoid unnecessary memory consumption of a record lock struct for a gap type lock.
        if heap_no == PAGE_HEAP_NO_SUPREMUM {
            ut_ad!(!Lock::is_lock_type(
                lock_mode_type.lock_type,
                LOCK_REC_NOT_GAP
            ));

            lock_mode_type.lock_type &= !(LOCK_GAP | LOCK_REC_NOT_GAP);
        }

        let page_id = buf_block.read().unwrap().get_page_id();

        // 1. Check if there is a waiting lock request on the same record or on gap.
        if let Some(locks) = self.rec_locks.get(&page_id) {}
    }

    /// Moves record locks from an old block to a new block.
    ///
    /// This function is used during page splits or merges when records are moved
    /// from one page to another.
    /// It transfers locks for all the records in the old block from the provided `rec` to the end of the list.
    ///
    /// # Arguments
    ///
    /// * `new_block` - The destination buffer block where records are being moved to
    /// * `old_block` - The source buffer block where records are being moved from
    /// * `rec` - The record being moved
    ///
    pub fn move_rec_list_end(
        &mut self,
        new_block: Arc<RwLock<BufBlock>>,
        old_block: Arc<RwLock<BufBlock>>,
        rec: Record,
    ) {
        let page_id = old_block.read().unwrap().get_page_id();
        let index = Arc::new(Index::new(page_id));
        let mut move_lock_requests: Vec<MoveLockRequest> = vec![];

        if let Some(locks) = self.rec_locks.get(&page_id) {
            for lock in locks {
                let mut lock_ref = lock.borrow_mut();
                let lock_mode_type = LockModeType {
                    lock_mode: lock_ref.lock_mode,
                    lock_type: lock_ref.lock_type,
                };
                let mut old_block_page_cur =
                    PageCur::new(Some(rec), old_block.clone());

                if old_block_page_cur.page_cur_is_before_first() {
                    old_block_page_cur.page_cur_move_to_next();
                }

                let mut new_block_page_cur =
                    PageCur::new(None, new_block.clone());
                new_block_page_cur.page_cur_move_to_next();

                // copy lock requests on user records to the new page, and reset the lock bits on the old.

                while old_block_page_cur.page_cur_is_after_last() == false {
                    let heap_no = old_block_page_cur
                        .page_cur_get_rec()
                        .rec_get_heap_no();

                    if lock_ref.rec_is_nth_bit_set(heap_no) {
                        lock_ref.rec_set_nth_bit(heap_no);

                        if Lock::is_lock_type(lock_ref.lock_type, LOCK_WAIT) {
                            lock_ref.reset();
                        }

                        let heap_no = new_block_page_cur
                            .page_cur_get_rec()
                            .rec_get_heap_no();

                        move_lock_requests.push(MoveLockRequest {
                            lock_mode_type: lock_mode_type.clone(),
                            heap_no,
                            trx: lock_ref.trx.clone(),
                        });
                    }

                    old_block_page_cur.page_cur_move_to_next();
                    new_block_page_cur.page_cur_move_to_next();
                }
            }
        }

        for move_lock_request in move_lock_requests {
            self.rec_add_to_queue(
                move_lock_request.lock_mode_type,
                new_block.clone(),
                move_lock_request.heap_no,
                index.clone(),
                move_lock_request.trx,
            );
        }
    }

    /// Moves record locks from an old block to a new block.
    ///
    /// This function is used during page splits or merges when records are moved
    /// from one page to another.
    /// It transfers locks for all the records in the old block from the start to the provided `rec`.
    ///
    /// # Arguments
    ///
    /// * `new_block` - The destination buffer block where records are being moved to
    /// * `old_block` - The source buffer block where records are being moved from
    /// * `rec` - The record being moved
    ///
    pub fn move_rec_list_start(
        &mut self,
        new_block: Arc<RwLock<BufBlock>>,
        old_block: Arc<RwLock<BufBlock>>,
        rec: Record,
    ) {
        let page_id = old_block.read().unwrap().get_page_id();
        let page = old_block.read().unwrap().get_page();
        let index = Arc::new(Index::new(page_id));
        let mut move_lock_requests: Vec<MoveLockRequest> = vec![];

        if let Some(locks) = self.rec_locks.get(&page_id) {
            for lock in locks {
                let mut lock_ref = lock.borrow_mut();
                let lock_mode_type = LockModeType {
                    lock_mode: lock_ref.lock_mode,
                    lock_type: lock_ref.lock_type,
                };
                let mut old_block_page_cur =
                    PageCur::new(Some(rec), old_block.clone());

                if old_block_page_cur.page_cur_is_before_first() {
                    old_block_page_cur.page_cur_move_to_next();
                }

                let mut new_block_page_cur =
                    PageCur::new(None, new_block.clone());
                new_block_page_cur.page_cur_move_to_next();

                // copy lock requests on user records to the new page, and reset the lock bits on the old.

                while old_block_page_cur.page_cur_get_rec() != rec {
                    let heap_no = old_block_page_cur
                        .page_cur_get_rec()
                        .rec_get_heap_no();

                    if lock_ref.rec_is_nth_bit_set(heap_no) {
                        lock_ref.rec_set_nth_bit(heap_no);

                        if Lock::is_lock_type(lock_ref.lock_type, LOCK_WAIT) {
                            lock_ref.reset();
                        }

                        let heap_no = new_block_page_cur
                            .page_cur_get_rec()
                            .rec_get_heap_no();

                        move_lock_requests.push(MoveLockRequest {
                            lock_mode_type: lock_mode_type.clone(),
                            heap_no,
                            trx: lock_ref.trx.clone(),
                        });
                    }

                    old_block_page_cur.page_cur_move_to_next();
                    new_block_page_cur.page_cur_move_to_next();
                }
            }
        }

        for move_lock_request in move_lock_requests {
            self.rec_add_to_queue(
                move_lock_request.lock_mode_type,
                new_block.clone(),
                move_lock_request.heap_no,
                index.clone(),
                move_lock_request.trx,
            );
        }
    }

    /// Checks if a lock exists for a specific page and heap number.
    ///
    /// This function determines whether any lock has been set for the given page ID and heap number.
    /// It's used to verify lock existence before performing operations like moving locks between records.
    ///
    /// # Arguments
    ///
    /// * `page_id` - The ID of the page containing the record
    /// * `heap_no` - The heap number of the record to check for locks
    ///
    /// # Returns
    ///
    /// * `true` if at least one lock exists for the specified record
    /// * `false` if no locks exist for the specified record
    fn rec_exists(&self, page_id: PageId, heap_no: usize) -> bool {
        if let Some(locks) = self.rec_locks.get(&page_id) {
            locks.iter().any(|lock| {
                let lock_ref = lock.borrow();
                lock_ref.rec_is_nth_bit_set(heap_no)
            })
        } else {
            false
        }
    }

    /// Moves all locks from a donor record to a receiver record.
    ///
    /// This function transfers all locks from a specific record in the donor block to a specific
    /// record in the receiver block.
    /// It performs the following operations:
    /// 1. Verifies that a lock exists for donor record
    /// 2. For each lock on the donor record:
    ///    - Resets the lock bit for the donor record
    ///    - Resets any waiting locks
    ///    - Creates a lock request to be applied to the receiver record
    /// 3. Adds new locks to the receiver record using the collected lock requests
    ///
    /// # Arguments
    ///
    /// * `receiver` - The buffer block containing the record that will receive the locks
    /// * `donor` - The buffer block containing the record that will donate its locks
    /// * `receiver_heap_no` - The heap number of the receiver record
    /// * `donor_heap_no` - The heap number of the donor record
    pub fn rec_move(
        &mut self,
        receiver: Arc<RwLock<BufBlock>>,
        donor: Arc<RwLock<BufBlock>>,
        receiver_heap_no: usize,
        donor_heap_no: usize,
    ) {
        let page_id = donor.read().unwrap().get_page_id();
        ut_ad!(self.rec_exists(page_id, donor_heap_no));

        let mut move_lock_requests: Vec<MoveLockRequest> = vec![];

        if let Some(locks) = self.rec_locks.get(&page_id) {
            for lock in locks {
                let mut lock_ref = lock.borrow_mut();

                let lock_mode_type = LockModeType {
                    lock_mode: lock_ref.lock_mode,
                    lock_type: lock_ref.lock_type,
                };

                if lock_ref.rec_is_nth_bit_set(donor_heap_no) {
                    lock_ref.rec_reset_nth_bit(donor_heap_no);
                }

                if Lock::is_lock_type(lock_ref.lock_type, LOCK_WAIT) {
                    lock_ref.reset();
                }

                let index = match &lock_ref.txn_lock {
                    TxnLock::RecLock(r) => r.index.clone(),
                    _ => {
                        ut_panic!("unexpected lock type");
                    }
                };

                move_lock_requests.push(MoveLockRequest {
                    lock_mode_type,
                    heap_no: receiver_heap_no,
                    trx: lock_ref.trx.clone(),
                });
            }
        }

        for lock_request in move_lock_requests {
            self.rec_add_to_queue(
                lock_request.lock_mode_type,
                receiver.clone(),
                lock_request.heap_no,
                Arc::new(Index::new(page_id)),
                lock_request.trx,
            );
        }
    }

    /// Update the lock table when a page has been re-organized.
    ///
    /// # Arguments
    /// * `new_buf_block` - The new buffer block that has the records.
    /// * `old_buf_block` - The old buffer block which is being reorganized.
    ///
    pub fn move_reorganize_page(
        &mut self,
        new_buf_block: Arc<RwLock<BufBlock>>,
        old_buf_block: Arc<RwLock<BufBlock>>,
    ) {
        let new_page = new_buf_block.read().unwrap().get_page();
        let old_page = old_buf_block.read().unwrap().get_page();

        let page_id = new_page.page_get_page_id();

        let mut old_locks: Vec<Rc<RefCell<Lock>>> = vec![];
        if let Some(locks) = self.rec_locks.get(&page_id) {
            for lock in locks {
                let mut lock_ref = lock.borrow_mut();

                if lock_ref.is_waiting() {
                    lock_ref.reset();
                }

                old_locks.push(lock.clone());
            }
        }

        let dummy_index = Arc::new(Index::new(page_id));
        let mut new_page_cur = PageCur::new(None, new_buf_block);
        let mut old_page_cur = PageCur::new(None, old_buf_block.clone());

        loop {
            let old_rec = old_page_cur.page_cur_get_rec();
            let old_heap_no = old_rec.rec_get_heap_no();

            let new_rec = new_page_cur.page_cur_get_rec();
            let new_heap_no = new_rec.rec_get_heap_no();

            for lock in &old_locks {
                let mut lock_ref = lock.borrow_mut();
                if lock_ref.rec_is_nth_bit_set(old_heap_no) {
                    lock_ref.rec_reset_nth_bit(old_heap_no);

                    lock_ref.rec_set_nth_bit(new_heap_no);

                    let index = match &lock_ref.txn_lock {
                        TxnLock::RecLock(rec) => rec.index.clone(),
                        TxnLock::TableLock(tbl) => {
                            ut_panic!("unexpected lock type");
                        }
                    };

                    self.rec_add_to_queue(
                        LockModeType {
                            lock_mode: lock_ref.lock_mode,
                            lock_type: lock_ref.lock_type,
                        },
                        old_buf_block.clone(),
                        new_heap_no,
                        index,
                        lock_ref.trx.clone(),
                    );

                    break;
                }
            }

            if old_page.page_rec_is_supremum(old_rec) {
                // We have iterated through all the records.
                ut_ad!(new_page.page_rec_is_supremum(new_rec));
                break;
            }

            old_page_cur.page_cur_move_to_next();
            new_page_cur.page_cur_move_to_next();
        }
    }

    /// Grant the lock to a waiting lock request and release the waiting transaction.
    ///
    /// # Arguments
    ///
    /// `lock`: A reference to the lock object.
    pub fn grant(&self, lock: &mut Lock) {
        lock.reset();

        // todo: Why do we need to check this.
        if lock.trx.trx_get_que_state() == TrxQueLockWait {
            lock.trx.end_lock_wait();
        }
    }

    /// Cancel the waiting record lock request and releases the waiting transaction.
    ///
    /// # Arguments
    ///
    /// `lock`: Reference to the lock object.
    ///
    pub fn rec_cancel(&mut self, lock: &mut Lock) {
        ut_ad!(Lock::is_lock_type(lock.lock_type, LOCK_REC));

        // 1. Reset the lock bit in the lock bitmap. There can be only one bit set.
        let nth_bit = lock.rec_find_set_bit();
        lock.rec_reset_nth_bit(some_or_panic!(nth_bit));

        // 2. Reset the wait flag.
        lock.reset();

        // 3. Release the trx from lock wait.
        lock.trx.end_lock_wait()
    }

    /// Enqueues a waiting request for a lock which cannot be granted immediately and checks for deadlocks.
    /// If a deadlock is detected, the lock request is removed and an error code is returned.
    ///
    /// This function creates a new lock in the waiting state and performs deadlock detection.
    /// If a deadlock is found, it cleans up the lock and returns an error. If no deadlock is found,
    /// it sets the transaction state to waiting for the lock.
    ///
    /// # Arguments
    ///
    /// * `lock_mode_type` - The mode and type of the lock being requested
    /// * `buf_block` - The buffer block containing the record to be locked
    /// * `heap_no` - The heap number of the record to be locked
    /// * `index` - The index to which the record belongs
    /// * `trx` - The transaction that is requesting the lock
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the lock request was successfully enqueued
    /// * `Err(DbErr::DbDeadlock)` if a deadlock was detected
    /// * `Err(DbErr::DbLockWait)` if the transaction must wait for the lock
    pub fn rec_enqueue_waiting(
        &mut self,
        lock_mode_type: LockModeType,
        buf_block: Arc<RwLock<BufBlock>>,
        heap_no: usize,
        index: Arc<Index>,
        trx: Arc<Trx>,
    ) -> Result<(), DbErr> {
        let lock_mode_type = LockModeType {
            lock_mode: lock_mode_type.lock_mode,
            lock_type: lock_mode_type.lock_type | LOCK_WAIT,
        };

        let page = buf_block.read().unwrap().get_page();
        let page_id = page.page_get_page_id();
        let n_bits = page.page_dir_get_n_heap();
        let lock = self.rec_create_low(
            page_id,
            lock_mode_type,
            heap_no,
            n_bits,
            index,
            trx.clone(),
        );

        // 1. Check if a deadlock occurs if we add.
        // todo: Use std::unlikely
        if self.deadlock_occurs(lock.clone(), trx.clone()) {
            let mut lock_ref = lock.borrow_mut();
            lock_ref.reset();
            lock_ref.rec_reset_nth_bit(heap_no);

            return Err(DbErr::Deadlock);
        } else if let Some(wait_lock) = trx.get_wait_lock() {
            trx.trx_set_que_state(TrxQueLockWait);
            ut_ad!(wait_lock == lock.borrow().id);

            return Err(DbErr::LockWait);
        }

        ut_ad!(trx.get_wait_lock().is_none());

        Ok(())
    }

    /// Remove the lock request, waiting or granted, from the queue.
    ///
    /// # Arguments
    ///
    /// `lock`: Lock request to be removed.
    ///
    pub fn rec_discard(&mut self, lock: &mut Lock) {
        ut_ad!(Lock::is_lock_type(lock.lock_type, LOCK_REC));

        let page_id = match &lock.txn_lock {
            TxnLock::RecLock(rec_lock) => rec_lock.page_id,
            _ => {
                ut_panic!("unexpected lock type");
            }
        };

        let ret = self.rec_locks.remove(&page_id);
        ut_ad!(ret.is_some());

        lock.trx.remove_lock(lock.id);
    }

    /// Create a table lock object and add it to the lock queue.
    /// This method does not check for deadlock or lock compatibility.
    ///
    /// # Arguments
    ///
    /// * `table` - The table to be locked
    /// * `lock_mode_type` - The mode and type of the lock being requested
    /// * `trx` - The transaction that is requesting the lock
    ///
    pub fn table_create(
        &mut self,
        table: Arc<Table>,
        lock_mode_type: LockModeType,
        trx: Arc<Trx>,
    ) {
        let lock_id = self.get_next_lock_id();
        let lock = Rc::new(RefCell::new(Lock::new(
            lock_id,
            trx.clone(),
            0, /* todo: Remove this.*/
            TxnLock::TableLock(TableLock { table: table.clone() }),
            lock_mode_type.lock_mode,
            lock_mode_type.lock_type | LOCK_TABLE,
        )));

        trx.add_lock(lock_id);
        if let Some(locks) = self.table_locks.get_mut(&table.get_id()) {
            locks.push(lock.clone());
        } else {
            self.table_locks.insert(table.get_id(), vec![lock.clone()]);
        }
    }

    /// Check if other transactions have an incompatible lock mode request in the lock queue.
    ///
    /// This function checks if there are any locks on the specified table from other transactions
    /// that are incompatible with the requested lock mode. A lock is considered incompatible if:
    /// 1. It belongs to a different transaction
    /// 2. Its lock mode is not compatible with the requested lock mode
    /// 3. It is not a waiting lock (only active locks are considered)
    ///
    /// # Arguments
    ///
    /// * `trx` - The transaction that is requesting the lock
    /// * `table` - The table to check for incompatible locks
    /// * `lock_mode_type` - The mode and type of the lock being requested
    ///
    /// # Returns
    ///
    /// * `Some(lock)` - If an incompatible lock is found, returns the first such lock
    /// * `None` - If no incompatible locks are found
    ///
    pub fn table_other_has_incompatible(
        &self,
        trx: Arc<Trx>,
        table: Arc<Table>,
        lock_mode_type: LockModeType,
    ) -> Option<Rc<RefCell<Lock>>> {
        if let Some(locks) = self.table_locks.get(&table.get_id()) {
            for lock in locks {
                let lock_ref = lock.borrow();
                if lock_ref.trx.trx_get_id() != trx.trx_get_id()
                    && !Lock::mode_compatible(
                        lock_ref.lock_mode,
                        lock_mode_type.lock_mode,
                    )
                    && !lock_ref.is_waiting()
                {
                    return Some(lock.clone());
                }
            }
        }

        None
    }

    /// Check if the waiting lock request has to still wait in the queue.
    ///
    /// This function determines if a waiting lock request for a table still needs to wait
    /// in the queue. It checks all locks on the specified table to see if any of them
    /// would block the waiting lock. A waiting lock needs to continue waiting if:
    /// 1. There is at least one lock ahead of it in the queue that it has to wait for
    /// 2. The waiting lock is not found in the queue (which should not happen)
    ///
    /// # Arguments
    ///
    /// * `wait_lock` - The waiting lock request to check
    ///
    /// # Returns
    ///
    /// * `true` - If the lock still needs to wait
    /// * `false` - If the lock no longer needs to wait
    ///
    pub fn table_has_to_wait_in_queue(&mut self, wait_lock: &Lock) -> bool {
        ut_ad!(wait_lock.is_waiting());

        let table_id = match &wait_lock.txn_lock {
            TxnLock::TableLock(table_lock) => table_lock.table.get_id(),
            TxnLock::RecLock(_) => {
                ut_panic!("unexpected lock type");
            }
        };

        let locks = self.table_locks.entry(table_id).or_insert(vec![]);

        for lock in locks {
            if lock.borrow().id == wait_lock.id {
                return false;
            }

            if wait_lock.has_to_wait_for(lock.borrow().deref(), None) {
                return true;
            }
        }

        false
    }

    /// Check if the transaction has the specified lock mode or stronger on a table.
    ///
    /// This function checks if the specified transaction has a lock on the given table
    /// with a lock mode that is equal to or stronger than the requested lock mode.
    /// It searches through all locks on the table and returns the first matching lock.
    ///
    /// # Arguments
    ///
    /// * `trx` - The transaction to check for locks
    /// * `table` - The table to check for locks
    /// * `lock_mode_type` - The mode and type of the lock to check for
    ///
    /// # Returns
    ///
    /// * `Some(lock)` - If a matching lock is found, returns the lock
    /// * `None` - If no matching lock is found
    ///
    pub fn table_has(
        &self,
        trx: Arc<Trx>,
        table: Arc<Table>,
        lock_mode_type: LockModeType,
    ) -> Option<Rc<RefCell<Lock>>> {
        if let Some(locks) = self.table_locks.get(&table.get_id()) {
            for lock in locks {
                let lock_ref = lock.borrow();
                if lock_ref.trx.trx_get_id() == trx.trx_get_id()
                    && Lock::mode_stronger_or_eq(
                        lock_ref.lock_mode(),
                        lock_mode_type.lock_mode,
                    )
                {
                    // The given transaction has already locked the table in equal or higher mode.

                    ut_ad!(lock_ref.is_waiting() == false);
                    return Some(lock.clone());
                }
            }
        }

        None
    }
}
