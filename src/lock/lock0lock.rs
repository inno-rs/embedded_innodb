use intrusive_collections::{LinkedListLink, intrusive_adapter};

use crate::buf::buf0types::BufBlock;
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::DbErr::DbFail;
use crate::innodb::innodb0types::{DbErr, TxnId};
use crate::lock::lock0types::{
    LOCK_COMPATIBILITY_MATRIX, LOCK_MODE_MASK, LockMode, LockType,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::rem::rem0rec::Record;
use crate::trx::trx0trx::Trx;

pub struct Lock {
    /// Transaction owning the lock.
    trx: Trx,
    /// List of locks on the same transaction.
    trx_locks: LinkedListLink,
    /// Lock mode.
    lock_mode: LockMode,
}

intrusive_adapter!(LockListAdapter = Box<Lock>: Lock { trx_locks: LinkedListLink });
impl Lock {
    pub fn new() -> Self {
        Lock {
            trx: Trx::new(0),
            trx_locks: Default::default(),
            lock_mode: LockMode::LOCK_NONE,
        }
    }

    /// Get the id of the transaction that owns the lock.
    ///
    /// # Returns
    ///
    /// The transaction id of the lock.
    pub fn trx_id(&self) -> TxnId {
        self.trx.trx_get_id()
    }

    pub fn lock_type(&self) -> LockType {
        let t = self.lock_mode as u16;
        t & LOCK_MODE_MASK
    }

    /// Determines if two locks modes are compatible.
    ///
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
    /// A new LockSys instance initialized with default settings.
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
