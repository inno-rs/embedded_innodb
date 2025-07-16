#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pretty_assertions::assert_eq;
    use rstest::{fixture, rstest};

    use crate::{
        dict::dict0types::{Index, Table},
        innodb::innodb0types::PageId,
        lock::{
            lock0lock::{Lock, RecLock, TableLock, TxnLock},
            lock0types::{
                LOCK_GAP, LOCK_INSERT_INTENTION, LOCK_REC_NOT_GAP, LockMode,
                REC_LOCK, TABLE_LOCK,
            },
        },
        trx::trx0trx::Trx,
    };

    #[fixture]
    fn trx() -> Arc<Trx> {
        Arc::new(Trx::new(1))
    }

    #[fixture]
    fn table() -> Arc<Mutex<Table>> {
        Arc::new(Mutex::new(Table {}))
    }

    #[fixture]
    fn index() -> Arc<Index> {
        Arc::new(Index::new(PageId { space_id: 0, page_no: 0 }))
    }

    #[fixture]
    fn page_id() -> PageId {
        PageId { space_id: 0, page_no: 0 }
    }

    #[fixture]
    fn table_lock(table: Arc<Mutex<Table>>) -> TableLock {
        TableLock { table }
    }

    #[fixture]
    fn rec_lock(index: Arc<Index>, page_id: PageId) -> RecLock {
        RecLock { index, page_id }
    }

    #[fixture]
    fn txn_lock_table(table_lock: TableLock) -> TxnLock {
        TxnLock::TableLock(table_lock)
    }

    #[fixture]
    fn txn_lock_rec(rec_lock: RecLock) -> TxnLock {
        TxnLock::RecLock(rec_lock)
    }

    #[rstest]
    fn test_lock_new(trx: Arc<Trx>, txn_lock_table: TxnLock) {
        // Test creating a new lock with LOCK_NONE mode and TABLE_LOCK type
        let lock = Lock::new(
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LOCK_NONE,
            TABLE_LOCK,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq, we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LOCK_NONE => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        assert_eq!(lock.trx_id(), trx.trx_get_id());
        assert_eq!(lock.lock_type(), TABLE_LOCK);
    }

    #[rstest]
    fn test_lock_type_and_mode(trx: Arc<Trx>, txn_lock_table: TxnLock) {
        // Create a lock with LOCK_NONE mode and TABLE_LOCK type
        let lock = Lock::new(
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LOCK_NONE,
            TABLE_LOCK,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq, we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LOCK_NONE => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        // Verify the lock type is TABLE_LOCK
        assert_eq!(lock.lock_type(), TABLE_LOCK);
    }

    #[rstest]
    fn test_lock_rec_lock(trx: Arc<Trx>, rec_lock: RecLock) {
        // Create a lock with LOCK_NONE mode and REC_LOCK type
        let lock = Lock::new(
            trx.clone(),
            20,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq, we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LOCK_NONE => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        // Verify the lock type is REC_LOCK
        assert_eq!(lock.lock_type(), REC_LOCK);
    }

    #[rstest]
    fn test_lock_gap_and_insert_intention(trx: Arc<Trx>, rec_lock: RecLock) {
        // Create a lock with LOCK_GAP type
        let gap_lock = Lock::new(
            trx.clone(),
            30,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LOCK_NONE,
            LOCK_GAP | REC_LOCK,
        );
        assert!(gap_lock.rec_is_gap());

        // Create a lock with LOCK_INSERT_INTENTION type
        let insert_intention_lock = Lock::new(
            trx.clone(),
            40,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            LOCK_INSERT_INTENTION | REC_LOCK,
        );
        assert!(insert_intention_lock.rec_is_insert_intention());
    }

    #[rstest]
    fn test_trx_id(trx: Arc<Trx>, txn_lock_table: TxnLock) {
        // Create a lock with a specific trx
        let lock = Lock::new(
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LOCK_NONE,
            TABLE_LOCK,
        );

        // Verify the trx_id of the lock
        assert_eq!(lock.trx_id(), trx.trx_get_id());
    }
    #[rstest]
    fn test_rec_is_gap(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            Arc::new(Trx::new(1)),
            60,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LOCK_NONE,
            LOCK_GAP | REC_LOCK,
        );

        // Verify if the lock is a gap lock
        assert!(lock.rec_is_gap());

        // Create a lock that is not a gap lock
        let non_gap_lock = Lock::new(
            Arc::new(Trx::new(2)),
            70,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Verify that it is not a gap lock
        assert!(!non_gap_lock.rec_is_gap());
    }
    #[rstest]
    fn test_rec_is_insert_intention(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            Arc::new(Trx::new(1)),
            80,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LOCK_NONE,
            LOCK_INSERT_INTENTION | REC_LOCK,
        );

        // Verify if the lock is an insert intention lock
        assert!(lock.rec_is_insert_intention());

        // Create a lock that is not an insert intention lock
        let non_insert_intention_lock = Lock::new(
            Arc::new(Trx::new(2)),
            90,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Verify that it is not an insert intention lock
        assert!(!non_insert_intention_lock.rec_is_insert_intention());
    }

    #[rstest]
    fn test_rec_is_not_gap(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            Arc::new(Trx::new(1)),
            80,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LOCK_NONE,
            LOCK_REC_NOT_GAP | REC_LOCK,
        );

        // Verify if the lock is an insert intention lock
        assert!(lock.rec_is_not_gap());

        // Create a lock that is not an insert intention lock
        let non_insert_intention_lock = Lock::new(
            Arc::new(Trx::new(2)),
            90,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Verify that it is not an insert intention lock
        assert_eq!(non_insert_intention_lock.rec_is_not_gap(), false);
    }

    #[rstest]
    fn test_rec_set_nth_bit(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Initially, all bits should be unset
        for i in 0..total_bits {
            assert!(!lock.rec_is_nth_bit_set(i));
        }

        // Set some bits
        lock.rec_set_nth_bit(10);
        lock.rec_set_nth_bit(20);

        // Verify the bits are set
        assert!(lock.rec_is_nth_bit_set(10));
        assert!(lock.rec_is_nth_bit_set(20));

        // Other bits should still be unset
        for i in 0..total_bits {
            if i != 10 && i != 20 {
                assert!(!lock.rec_is_nth_bit_set(i));
            }
        }
    }

    #[rstest]
    fn test_rec_get_n_bits(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Get the number of set bits
        let expected = lock.rec_get_n_bits();
        assert_eq!(expected, total_bits); // Should return the count of bits
    }

    #[rstest]
    fn test_rec_find_set_bit(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Set some bits
        lock.rec_set_nth_bit(10);
        lock.rec_set_nth_bit(20);

        // Find the first set bit
        let first_set_bit = lock.rec_find_set_bit();
        assert_eq!(first_set_bit, Some(10)); // Should return the first set bit

        // Reset the nth bit and find again
        lock.rec_reset_nth_bit(10);
        let next_set_bit = lock.rec_find_set_bit();
        assert_eq!(next_set_bit, Some(20)); // Should return the next set bit
    }

    #[rstest]
    fn test_rec_is_nth_bit_set(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Set some bits
        lock.rec_set_nth_bit(10);
        lock.rec_set_nth_bit(20);

        // Check if the nth bit is set
        assert!(lock.rec_is_nth_bit_set(10));
        assert!(lock.rec_is_nth_bit_set(20));
        assert!(!lock.rec_is_nth_bit_set(30)); // Assuming 30 is not set

        // Check for bits that are not set
        for i in 0..total_bits {
            if i == 10 || i == 20 {
                assert!(lock.rec_is_nth_bit_set(i));
            } else {
                assert!(!lock.rec_is_nth_bit_set(i));
            }
        }
    }

    #[rstest]
    fn test_rec_reset_nth_bit(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Set some bits
        lock.rec_set_nth_bit(10);
        lock.rec_set_nth_bit(20);

        // Reset the nth bit
        lock.rec_reset_nth_bit(10);

        // Check if the nth bit is reset
        assert!(!lock.rec_is_nth_bit_set(10));
        assert!(lock.rec_is_nth_bit_set(20));
    }

    #[rstest]
    fn test_rec_bitmap_reset(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Set some bits
        lock.rec_set_nth_bit(10);
        lock.rec_set_nth_bit(20);

        // Reset the bitmap
        lock.rec_bitmap_reset();

        // Check if all bits are reset
        for i in 0..total_bits {
            assert!(!lock.rec_is_nth_bit_set(i));
        }
    }

    #[rstest]
    fn test_rec_page_id(rec_lock: RecLock, page_id: PageId) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            Arc::new(Trx::new(1)),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_NONE,
            REC_LOCK,
        );

        // Verify the page_id of the lock
        assert_eq!(lock.rec_page_id(), page_id);
    }

    #[test]
    fn test_mode_compatible() {
        // Test compatibility matrix
        assert!(Lock::mode_compatible(LockMode::LOCK_IS, LockMode::LOCK_IS));
        assert!(Lock::mode_compatible(LockMode::LOCK_IS, LockMode::LOCK_IX));
        assert!(Lock::mode_compatible(LockMode::LOCK_IS, LockMode::LOCK_S));
        assert!(!Lock::mode_compatible(LockMode::LOCK_IS, LockMode::LOCK_X));

        assert!(Lock::mode_compatible(LockMode::LOCK_IX, LockMode::LOCK_IS));
        assert!(Lock::mode_compatible(LockMode::LOCK_IX, LockMode::LOCK_IX));
        assert!(!Lock::mode_compatible(LockMode::LOCK_IX, LockMode::LOCK_S));
        assert!(!Lock::mode_compatible(LockMode::LOCK_IX, LockMode::LOCK_X));

        assert!(Lock::mode_compatible(LockMode::LOCK_S, LockMode::LOCK_IS));
        assert!(!Lock::mode_compatible(LockMode::LOCK_S, LockMode::LOCK_IX));
        assert!(Lock::mode_compatible(LockMode::LOCK_S, LockMode::LOCK_S));
        assert!(!Lock::mode_compatible(LockMode::LOCK_S, LockMode::LOCK_X));

        assert!(!Lock::mode_compatible(LockMode::LOCK_X, LockMode::LOCK_IS));
        assert!(!Lock::mode_compatible(LockMode::LOCK_X, LockMode::LOCK_IX));
        assert!(!Lock::mode_compatible(LockMode::LOCK_X, LockMode::LOCK_S));
        assert!(!Lock::mode_compatible(LockMode::LOCK_X, LockMode::LOCK_X));
    }

    #[rstest]
    fn test_rec_blocks_same_transaction(rec_lock: RecLock) {
        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Create a lock with the same transaction ID
        let lock = Lock::new(
            trx.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK,
        );

        // Create a lock mode type for testing
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK,
        };

        // Test with the same transaction - should not block
        assert!(!lock.rec_blocks(trx.clone(), lock_mode_type, false));
    }

    #[rstest]
    fn test_rec_blocks_compatible_modes(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with IS mode
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_IS,
            REC_LOCK,
        );

        // Create a lock mode type with IS mode (compatible with IS)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_IS,
            lock_type: REC_LOCK,
        };

        // Test with compatible lock modes - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }

    #[rstest]
    fn test_rec_blocks_incompatible_gap_lock(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and GAP type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK | LOCK_GAP,
        };

        // Test with incompatible lock modes but gap lock - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }

    #[rstest]
    fn test_rec_blocks_gap_lock_on_gap_lock(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and GAP type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK | LOCK_GAP,
        };

        // Test with incompatible lock modes but both gap locks - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }

    #[rstest]
    fn test_rec_blocks_insert_intention_on_gap_lock(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and GAP type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and INSERT_INTENTION type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK | LOCK_INSERT_INTENTION,
        };

        // Test with incompatible lock modes, insert intention on gap lock
        assert!(lock.rec_blocks(trx2.clone(), lock_mode_type, false));
    }

    #[rstest]
    fn test_rec_blocks_on_supremum(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK,
        );

        // Create a lock mode type with S mode (incompatible with X)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK,
        };

        // Test with lock on supremum record
        assert_eq!(lock.rec_blocks(trx2.clone(), lock_mode_type, true), false);
    }

    #[rstest]
    fn test_rec_blocks_incompatible_modes_should_block(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and LOCK_REC_NOT_GAP type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_REC_NOT_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and LOCK_REC_NOT_GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK | LOCK_REC_NOT_GAP,
        };

        // Test with incompatible lock modes and non-gap locks - should block
        assert!(lock.rec_blocks(trx2.clone(), lock_mode_type, false));
    }

    #[rstest]
    fn test_rec_blocks_non_insert_intention_on_gap_lock(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and GAP type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and non-INSERT_INTENTION type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK,
        };

        // Test with incompatible lock modes, non-insert intention on gap lock - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }

    #[rstest]
    fn test_rec_blocks_insert_intention_lock_blocks(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with X mode and INSERT_INTENTION type
        let lock = Lock::new(
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LOCK_X,
            REC_LOCK | LOCK_INSERT_INTENTION,
        );

        // Create a lock mode type with S mode (incompatible with X)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LOCK_S,
            lock_type: REC_LOCK,
        };

        // Test with incompatible lock modes, current lock is insert intention - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }
}
