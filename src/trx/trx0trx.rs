use std::{collections::HashSet, sync::Mutex};

use crate::{
    innodb::innodb0types::TrxId,
    lock::lock0types::LockId,
    trx::trx0types::{TrxQueState, UndoNo},
    ut::ut0dbg::ut_a,
};

pub struct Trx {
    /// Id of the transaction.
    id: TrxId,
    /// Inner mutable fields of the transaction.
    inner: Mutex<TrxInner>,
}

pub struct TrxInner {
    /// Mark to indicate if the transaction is deadlocked.
    deadlock_mark: bool,
    /// Transaction queue state.
    que_state: TrxQueState,
    /// If the transaction execution state is TRX_QUEUE_LOCK_WAIT, this points to the lock request; otherwise it is a None.
    wait_lock: Option<LockId>,
    /// Next undo log record number to assign.
    /// Since the undo log is private for a transaction, this is a simple ascending sequence with no gaps.
    /// Thus, it represents the number of modified/inserted rows in a transaction.
    undo_no: UndoNo,
    /// All the locks reserved by the transaction.
    trx_locks: HashSet<LockId>,
}

impl Trx {
    pub fn new(id: TrxId) -> Self {
        Trx {
            id,
            inner: Mutex::new(TrxInner {
                wait_lock: None,
                que_state: TrxQueState::TrxQueRunning,
                deadlock_mark: false,
                undo_no: 0,
                trx_locks: Default::default(),
            }),
        }
    }

    /// Get the id of the transaction.
    ///
    /// # Returns
    ///
    /// `TnxId` of the transaction.
    pub fn trx_get_id(&self) -> TrxId {
        self.id
    }

    /// Set the wait lock of the transaction.
    ///
    /// # Arguments
    ///
    /// `lock`: The lock object to wait for.
    ///
    pub fn set_wait_lock(&self, lock_id: Option<LockId>) {
        let mut inner_ref = self.inner.lock().unwrap();
        match lock_id {
            None => {
                inner_ref.wait_lock = None;
            }
            Some(lock_id) => {
                inner_ref.wait_lock = Some(lock_id);
            }
        }
    }

    /// Get the lock that this transaction is waiting for, if any.
    ///
    /// # Returns
    ///
    /// `Option<LockId>` containing the ID of the lock this transaction is waiting for,
    /// or `None` if the transaction is not waiting for any lock.
    pub fn get_wait_lock(&self) -> Option<LockId> {
        self.inner.lock().unwrap().wait_lock
    }

    /// Get the transaction state.
    ///
    /// # Returns
    ///
    /// `TrxQueState` instance.
    pub fn trx_get_que_state(&self) -> TrxQueState {
        self.inner.lock().unwrap().que_state.clone()
    }

    /// Set the transaction queue state.
    ///
    /// # Arguments
    ///
    /// * `state` - The new state to set for the transaction
    pub fn trx_set_que_state(&self, state: TrxQueState) {
        self.inner.lock().unwrap().que_state = state;
    }

    /// Check if the transaction has been marked as part of a deadlock.
    ///
    /// During deadlock detection, transactions involved in the deadlock cycle
    /// are marked to indicate they are part of a deadlock.
    ///
    /// # Returns
    ///
    /// `true` if the transaction is marked as part of a deadlock, `false` otherwise
    pub fn trx_is_deadlock_mark(&self) -> bool {
        self.inner.lock().unwrap().deadlock_mark
    }

    /// Set the deadlock mark for this transaction.
    ///
    /// This method is used during deadlock detection to mark transactions
    /// that are part of a deadlock cycle.
    ///
    /// # Arguments
    ///
    /// * `deadlock_mark` - `true` to mark the transaction as part of a deadlock,
    ///                     `false` to clear the mark
    pub fn trx_set_deadlock_mark(&self, deadlock_mark: bool) {
        self.inner.lock().unwrap().deadlock_mark = deadlock_mark;
    }

    /// Remove the lock from the list of locks reserved by the transaction.
    ///
    /// This method is called when a lock is released or when a lock request is removed
    /// from the queue.
    ///
    /// # Arguments
    ///
    /// * `lock_id` - The ID of the lock to remove from the transaction's lock list
    ///
    pub fn remove_lock(&self, lock_id: LockId) {
        let mut inner = self.inner.lock().unwrap();
        let result = inner.trx_locks.remove(&lock_id);

        ut_a!(result);
    }

    /// Adds the lock from the list of locks reserved by the transaction.
    ///
    /// # Arguments
    ///
    /// * `lock_id` - The ID of the lock to remove from the transaction's lock list
    ///
    pub fn add_lock(&self, lock_id: LockId) {
        let mut inner = self.inner.lock().unwrap();
        let result = inner.trx_locks.insert(lock_id);

        ut_a!(result);
    }

    /// The transaction must be in the TrxQueLockWait state.
    /// Put it to TrxQueRunning state and releases all the threads that are waiting for it.
    ///
    pub fn end_lock_wait(&self) {
        let que_state = &mut self.inner.lock().unwrap().que_state;
        ut_a!(*que_state == TrxQueState::TrxQueLockWait);

        *que_state = TrxQueState::TrxQueRunning;
    }

    /// Get the undo no of the transaction.
    pub fn get_undo_no(&self) -> UndoNo {
        self.inner.lock().unwrap().undo_no
    }
}
