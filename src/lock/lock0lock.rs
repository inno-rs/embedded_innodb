use crate::innodb::innodb0types::TxnId;
use crate::lock::lock0types::{
    LockMode, LockType, LOCK_COMPATIBILITY_MATRIX, LOCK_MODE_MASK,
};
use crate::trx::trx0trx::Trx;
use intrusive_collections::{intrusive_adapter, LinkedListLink};

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
