#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use rstest::{fixture, rstest};

    use crate::{
        innodb::innodb0types::TrxId,
        lock::lock0types::LockId,
        trx::{trx0trx::Trx, trx0types::TrxQueState},
    };

    #[fixture]
    fn trx() -> Arc<Trx> {
        Arc::new(Trx::new(TrxId(1)))
    }

    #[rstest]
    fn test_trx_new() {
        let trx = Trx::new(TrxId(42));

        // Verify the transaction has the expected ID
        assert_eq!(trx.trx_get_id(), TrxId(42));

        // Verify the transaction is in the running state
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueRunning);

        // Verify the transaction is not marked as deadlocked
        assert!(!trx.trx_is_deadlock_mark());

        // Verify the transaction is not waiting for any lock
        assert_eq!(trx.get_wait_lock(), None);
    }

    #[rstest]
    fn test_trx_get_id(trx: Arc<Trx>) {
        // Verify the transaction ID is as expected
        assert_eq!(trx.trx_get_id(), TrxId(1));
    }

    #[rstest]
    fn test_set_wait_lock(trx: Arc<Trx>) {
        // Initially, the transaction should not be waiting for any lock
        assert_eq!(trx.get_wait_lock(), None);

        // Set a wait lock
        let lock_id = LockId(42);
        trx.set_wait_lock(Some(lock_id));

        // Verify the wait lock is set correctly
        assert_eq!(trx.get_wait_lock(), Some(lock_id));

        // Clear the wait lock
        trx.set_wait_lock(None);

        // Verify the wait lock is cleared
        assert_eq!(trx.get_wait_lock(), None);
    }

    #[rstest]
    fn test_get_wait_lock(trx: Arc<Trx>) {
        // Initially, the transaction should not be waiting for any lock
        assert_eq!(trx.get_wait_lock(), None);

        // Set a wait lock
        let lock_id = LockId(42);
        trx.set_wait_lock(Some(lock_id));

        // Verify the wait lock is set correctly
        assert_eq!(trx.get_wait_lock(), Some(lock_id));
    }

    #[rstest]
    fn test_trx_get_que_state(trx: Arc<Trx>) {
        // Initially, the transaction should be in the running state
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueRunning);

        // Change the state
        trx.trx_set_que_state(TrxQueState::TrxQueLockWait);

        // Verify the state is changed
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueLockWait);
    }

    #[rstest]
    fn test_trx_set_que_state(trx: Arc<Trx>) {
        // Initially, the transaction should be in the running state
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueRunning);

        // Change the state to lock wait
        trx.trx_set_que_state(TrxQueState::TrxQueLockWait);

        // Verify the state is changed to lock wait
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueLockWait);

        // Change the state back to running
        trx.trx_set_que_state(TrxQueState::TrxQueRunning);

        // Verify the state is changed back to running
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueRunning);
    }

    #[rstest]
    fn test_trx_is_deadlock_mark(trx: Arc<Trx>) {
        // Initially, the transaction should not be marked as deadlocked
        assert!(!trx.trx_is_deadlock_mark());

        // Mark the transaction as deadlocked
        trx.trx_set_deadlock_mark(true);

        // Verify the transaction is marked as deadlocked
        assert!(trx.trx_is_deadlock_mark());
    }

    #[rstest]
    fn test_trx_set_deadlock_mark(trx: Arc<Trx>) {
        // Initially, the transaction should not be marked as deadlocked
        assert!(!trx.trx_is_deadlock_mark());

        // Mark the transaction as deadlocked
        trx.trx_set_deadlock_mark(true);

        // Verify the transaction is marked as deadlocked
        assert!(trx.trx_is_deadlock_mark());

        // Unmark the transaction
        trx.trx_set_deadlock_mark(false);

        // Verify the transaction is not marked as deadlocked
        assert!(!trx.trx_is_deadlock_mark());
    }

    #[rstest]
    fn test_add_and_remove_lock(trx: Arc<Trx>) {
        let lock_id = LockId(42);

        // Add a lock to the transaction
        trx.add_lock(lock_id);

        // Remove the lock from the transaction
        trx.remove_lock(lock_id);

        // Adding the same lock again should succeed
        // (if remove_lock worked correctly)
        trx.add_lock(lock_id);
    }

    #[rstest]
    #[should_panic(expected = "assertion failed")]
    fn test_remove_non_existent_lock(trx: Arc<Trx>) {
        // Attempt to remove a lock that doesn't exist
        // This should panic with an assertion failure
        trx.remove_lock(LockId(42));
    }

    #[rstest]
    #[should_panic(expected = "assertion failed")]
    fn test_add_existing_lock(trx: Arc<Trx>) {
        let lock_id = LockId(42);

        // Add a lock to the transaction
        trx.add_lock(lock_id);

        // Attempt to add the same lock again
        // This should panic with an assertion failure
        trx.add_lock(lock_id);
    }

    #[rstest]
    fn test_end_lock_wait(trx: Arc<Trx>) {
        // Set the transaction state to lock wait
        trx.trx_set_que_state(TrxQueState::TrxQueLockWait);

        // End the lock wait
        trx.end_lock_wait();

        // Verify the transaction state is changed to running
        assert_eq!(trx.trx_get_que_state(), TrxQueState::TrxQueRunning);
    }

    #[rstest]
    #[should_panic(expected = "assertion failed")]
    fn test_end_lock_wait_not_in_lock_wait_state(trx: Arc<Trx>) {
        // The transaction is already in the running state
        // Calling end_lock_wait should panic with an assertion failure
        trx.end_lock_wait();
    }
}
