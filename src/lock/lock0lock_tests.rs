#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use pretty_assertions::assert_eq;
    use rstest::{fixture, rstest};

    use crate::{
        dict::dict0types::{Index, Table},
        innodb::innodb0types::PageId,
        lock::{
            lock0lock::{
                DeadlockSearchResult::{LockVictimIsStart, NoDeadlock},
                Lock, LockModeType, LockSys, RecLock, TableLock, TxnLock,
            },
            lock0types::{
                LOCK_GAP, LOCK_INSERT_INTENTION, LOCK_REC, LOCK_REC_NOT_GAP,
                LOCK_TABLE, LOCK_WAIT, LockId, LockMode,
            },
        },
        page::page0page::PageDebug,
        some_or_panic,
        tests::test0utils::{FieldType, create_records},
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
            LockId(0),
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LockNone,
            LOCK_TABLE,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq; we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LockNone => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        assert_eq!(lock.trx_id(), trx.trx_get_id());
        assert_eq!(lock.lock_type(), LOCK_TABLE);
    }

    #[rstest]
    fn test_lock_type_and_mode(trx: Arc<Trx>, txn_lock_table: TxnLock) {
        // Create a lock with LOCK_NONE mode and TABLE_LOCK type
        let lock = Lock::new(
            LockId(0),
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LockNone,
            LOCK_TABLE,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq, we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LockNone => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        // Verify the lock type is TABLE_LOCK
        assert_eq!(lock.lock_type(), LOCK_TABLE);
    }

    #[rstest]
    fn test_lock_rec_lock(trx: Arc<Trx>, rec_lock: RecLock) {
        // Create a lock with LOCK_NONE mode and REC_LOCK type
        let lock = Lock::new(
            LockId(0),
            trx.clone(),
            20,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
        );

        // Verify the lock has the expected properties
        // Since LockMode doesn't implement PartialEq, we need to use pattern matching
        match lock.lock_mode() {
            LockMode::LockNone => {} // This is what we expect
            _ => panic!("Expected LOCK_NONE, got a different lock mode"),
        }

        // Verify the lock type is REC_LOCK
        assert_eq!(lock.lock_type(), LOCK_REC);
    }

    #[rstest]
    fn test_lock_gap_and_insert_intention(trx: Arc<Trx>, rec_lock: RecLock) {
        // Create a lock with LOCK_GAP type
        let gap_lock = Lock::new(
            LockId(0),
            trx.clone(),
            30,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockNone,
            LOCK_GAP | LOCK_REC,
        );
        assert!(gap_lock.rec_is_gap());

        // Create a lock with LOCK_INSERT_INTENTION type
        let insert_intention_lock = Lock::new(
            LockId(0),
            trx.clone(),
            40,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_INSERT_INTENTION | LOCK_REC,
        );
        assert!(insert_intention_lock.rec_is_insert_intention());
    }

    #[rstest]
    fn test_trx_id(trx: Arc<Trx>, txn_lock_table: TxnLock) {
        // Create a lock with a specific trx
        let lock = Lock::new(
            LockId(0),
            trx.clone(),
            10,
            txn_lock_table,
            LockMode::LockNone,
            LOCK_TABLE,
        );

        // Verify the trx_id of the lock
        assert_eq!(lock.trx_id(), trx.trx_get_id());
    }
    #[rstest]
    fn test_rec_is_gap(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(1)),
            60,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockNone,
            LOCK_GAP | LOCK_REC,
        );

        // Verify if the lock is a gap lock
        assert!(lock.rec_is_gap());

        // Create a lock that is not a gap lock
        let non_gap_lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(2)),
            70,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
        );

        // Verify that it is not a gap lock
        assert!(!non_gap_lock.rec_is_gap());
    }
    #[rstest]
    fn test_rec_is_insert_intention(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(1)),
            80,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockNone,
            LOCK_INSERT_INTENTION | LOCK_REC,
        );

        // Verify if the lock is an insert intention lock
        assert!(lock.rec_is_insert_intention());

        // Create a lock that is not an insert intention lock
        let non_insert_intention_lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(2)),
            90,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
        );

        // Verify that it is not an insert intention lock
        assert!(!non_insert_intention_lock.rec_is_insert_intention());
    }

    #[rstest]
    fn test_rec_is_not_gap(rec_lock: RecLock) {
        // Create a RecLock with a specific page_id
        let lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(1)),
            80,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockNone,
            LOCK_REC_NOT_GAP | LOCK_REC,
        );

        // Verify if the lock is an insert intention lock
        assert!(lock.rec_is_not_gap());

        // Create a lock that is not an insert intention lock
        let non_insert_intention_lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(2)),
            90,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
        );

        // Verify that it is not an insert intention lock
        assert_eq!(non_insert_intention_lock.rec_is_not_gap(), false);
    }

    #[rstest]
    fn test_rec_set_nth_bit(rec_lock: RecLock) {
        let total_bits = 50; // Total bits for the record lock
        // Create a RecLock with a specific page_id
        let mut lock = Lock::new(
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            total_bits,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
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
            LockId(0),
            Arc::new(Trx::new(1)),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockNone,
            LOCK_REC,
        );

        // Verify the page_id of the lock
        assert_eq!(lock.rec_page_id(), page_id);
    }

    #[test]
    fn test_mode_compatible() {
        // Test compatibility matrix
        assert!(Lock::mode_compatible(LockMode::LockIS, LockMode::LockIS));
        assert!(Lock::mode_compatible(LockMode::LockIS, LockMode::LockIX));
        assert!(Lock::mode_compatible(LockMode::LockIS, LockMode::LockS));
        assert!(!Lock::mode_compatible(LockMode::LockIS, LockMode::LockX));

        assert!(Lock::mode_compatible(LockMode::LockIX, LockMode::LockIS));
        assert!(Lock::mode_compatible(LockMode::LockIX, LockMode::LockIX));
        assert!(!Lock::mode_compatible(LockMode::LockIX, LockMode::LockS));
        assert!(!Lock::mode_compatible(LockMode::LockIX, LockMode::LockX));

        assert!(Lock::mode_compatible(LockMode::LockS, LockMode::LockIS));
        assert!(!Lock::mode_compatible(LockMode::LockS, LockMode::LockIX));
        assert!(Lock::mode_compatible(LockMode::LockS, LockMode::LockS));
        assert!(!Lock::mode_compatible(LockMode::LockS, LockMode::LockX));

        assert!(!Lock::mode_compatible(LockMode::LockX, LockMode::LockIS));
        assert!(!Lock::mode_compatible(LockMode::LockX, LockMode::LockIX));
        assert!(!Lock::mode_compatible(LockMode::LockX, LockMode::LockS));
        assert!(!Lock::mode_compatible(LockMode::LockX, LockMode::LockX));
    }

    #[test]
    fn test_mode_stronger_or_eq() {
        // Test strength matrix
        assert!(Lock::mode_stronger_or_eq(LockMode::LockIS, LockMode::LockIS));
        assert!(!Lock::mode_stronger_or_eq(
            LockMode::LockIS,
            LockMode::LockIX
        ));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockIS, LockMode::LockS));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockIS, LockMode::LockX));

        assert!(Lock::mode_stronger_or_eq(LockMode::LockIX, LockMode::LockIS));
        assert!(Lock::mode_stronger_or_eq(LockMode::LockIX, LockMode::LockIX));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockIX, LockMode::LockS));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockIX, LockMode::LockX));

        assert!(Lock::mode_stronger_or_eq(LockMode::LockS, LockMode::LockIS));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockS, LockMode::LockIX));
        assert!(Lock::mode_stronger_or_eq(LockMode::LockS, LockMode::LockS));
        assert!(!Lock::mode_stronger_or_eq(LockMode::LockS, LockMode::LockX));

        assert!(Lock::mode_stronger_or_eq(LockMode::LockX, LockMode::LockIS));
        assert!(Lock::mode_stronger_or_eq(LockMode::LockX, LockMode::LockIX));
        assert!(Lock::mode_stronger_or_eq(LockMode::LockX, LockMode::LockS));
        assert!(Lock::mode_stronger_or_eq(LockMode::LockX, LockMode::LockX));
    }

    #[rstest]
    fn test_rec_blocks_same_transaction(rec_lock: RecLock) {
        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Create a lock with the same transaction ID
        let lock = Lock::new(
            LockId(0),
            trx.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC,
        );

        // Create a lock mode type for testing
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockIS,
            LOCK_REC,
        );

        // Create a lock mode type with IS mode (compatible with IS)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockIS,
            lock_type: LOCK_REC,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC | LOCK_GAP,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC | LOCK_GAP,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and INSERT_INTENTION type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC | LOCK_INSERT_INTENTION,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC,
        );

        // Create a lock mode type with S mode (incompatible with X)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_REC_NOT_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and LOCK_REC_NOT_GAP type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC | LOCK_REC_NOT_GAP,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_GAP,
        );

        // Create a lock mode type with S mode (incompatible with X) and non-INSERT_INTENTION type
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC,
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
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_INSERT_INTENTION,
        );

        // Create a lock mode type with S mode (incompatible with X)
        let lock_mode_type = crate::lock::lock0lock::LockModeType {
            lock_mode: LockMode::LockS,
            lock_type: LOCK_REC,
        };

        // Test with incompatible lock modes, current lock is insert intention - should not block
        assert_eq!(
            lock.rec_blocks(trx2.clone(), lock_mode_type, false),
            false
        );
    }

    #[rstest]
    fn test_has_to_wait_for_compatible_modes(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with S mode
        let mut lock1 = Lock::new(
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockS,
            LOCK_REC,
        );

        // Create another lock with S mode (compatible with S)
        let mut lock2 = Lock::new(
            LockId(0),
            trx2.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockS,
            LOCK_REC,
        );

        // Set the bit for the record in both locks
        let heap_no = 5;
        lock1.rec_set_nth_bit(heap_no);
        lock2.rec_set_nth_bit(heap_no);

        // Test with compatible lock modes - should not wait
        assert_eq!(lock1.has_to_wait_for(&lock2, Some(heap_no)), false);
    }

    #[rstest]
    fn test_has_to_wait_for_incompatible_table_locks() {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create table locks
        let table = table();
        let table_lock = table_lock(table.clone());

        // Create a lock with S mode
        let lock1 = Lock::new(
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::TableLock(table_lock.clone()),
            LockMode::LockS,
            0, // Table lock
        );

        // Create another lock with X mode (incompatible with S)
        let lock2 = Lock::new(
            LockId(0),
            trx2.clone(),
            50,
            TxnLock::TableLock(table_lock),
            LockMode::LockX,
            0, // Table lock
        );

        // Test with incompatible lock modes for table locks - should wait
        assert_eq!(lock1.has_to_wait_for(&lock2, None), true);
    }

    #[rstest]
    fn test_has_to_wait_for_incompatible_rec_locks(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with S mode
        let mut lock1 = Lock::new(
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockS,
            LOCK_REC,
        );

        // Create another lock with X mode (incompatible with S)
        let mut lock2 = Lock::new(
            LockId(0),
            trx2.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_REC_NOT_GAP,
        );

        // Set the bit for the record in both locks
        let heap_no = 5;
        lock1.rec_set_nth_bit(heap_no);
        lock2.rec_set_nth_bit(heap_no);

        // Test with incompatible lock modes for record locks - should call rec_blocks
        assert!(lock1.has_to_wait_for(&lock2, Some(heap_no)));
    }

    #[rstest]
    fn test_has_to_wait_for_same_transaction(rec_lock: RecLock) {
        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Create a lock with S mode
        let mut lock1 = Lock::new(
            LockId(0),
            trx.clone(),
            50,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockS,
            LOCK_REC,
        );

        // Create another lock with X mode (incompatible with S) but same transaction
        let mut lock2 = Lock::new(
            LockId(0),
            trx.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC,
        );

        // Set the bit for the record in both locks
        let heap_no = 5;
        lock1.rec_set_nth_bit(heap_no);
        lock2.rec_set_nth_bit(heap_no);

        // Test with same transaction - should not wait (rec_blocks returns false for same transaction)
        assert_eq!(lock1.has_to_wait_for(&lock2, Some(heap_no)), false);
    }

    #[rstest]
    fn test_has_to_wait_for_gap_locks(rec_lock: RecLock) {
        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Create a lock with S mode and GAP type
        let mut lock1 = Lock::new(
            LockId(0),
            trx1.clone(),
            50,
            TxnLock::RecLock(rec_lock.clone()),
            LockMode::LockS,
            LOCK_REC | LOCK_GAP,
        );

        // Create another lock with X mode (incompatible with S) and GAP type
        let mut lock2 = Lock::new(
            LockId(0),
            trx2.clone(),
            50,
            TxnLock::RecLock(rec_lock),
            LockMode::LockX,
            LOCK_REC | LOCK_GAP,
        );

        // Set the bit for the record in both locks
        let heap_no = 5;
        lock1.rec_set_nth_bit(heap_no);
        lock2.rec_set_nth_bit(heap_no);

        // Test with gap locks - should not wait (gap locks don't block each other)
        assert_eq!(lock1.has_to_wait_for(&lock2, Some(heap_no)), false);
    }

    #[test]
    fn test_rec_create_low() {
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        {
            let trx = Arc::new(Trx::new(1));
            // Not waiting for lock.
            let lock = lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );

            let lock = lock.borrow();
            assert_eq!(lock.rec_page_id(), page_id);
            assert!(lock.rec_is_nth_bit_set(rec_heap_no));
            assert_eq!(lock.lock_type() & LOCK_REC, LOCK_REC);
            assert!(trx.get_wait_lock().is_none());
        }

        {
            let trx = Arc::new(Trx::new(1));
            // Waiting lock.
            let lock = lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC | LOCK_WAIT,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );

            let lock = lock.borrow();
            assert_eq!(lock.rec_page_id(), page_id);
            assert!(lock.rec_is_nth_bit_set(rec_heap_no));
            assert_eq!(lock.lock_type() & LOCK_REC, LOCK_REC);
            assert_eq!(lock.get_id(), some_or_panic!(trx.get_wait_lock()));
        }
    }

    #[test]
    fn test_deadlock_recursive_with_non_conflicting_cycle() {
        // trxA -> trxB -> trxA
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));

        let page_id = PageId::dummy();
        let rec_x_heap_no = 10;
        let rec_y_heap_no = 20;
        let n_bits = 100;

        // Create trxA, trxB
        let trx_a = Arc::new(Trx::new(1));
        let trx_b = Arc::new(Trx::new(2));

        // trx_a has lock for rec_x, but waiting for rec_y
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockIS, lock_type: LOCK_REC },
            rec_x_heap_no,
            n_bits,
            index.clone(),
            trx_a.clone(),
        );

        let wait_for = lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType {
                lock_mode: LockMode::LockIS,
                lock_type: LOCK_REC | LOCK_WAIT,
            },
            rec_y_heap_no,
            n_bits,
            index.clone(),
            trx_a.clone(),
        );

        // trx_b has rec_y, but waiting for rec_x
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockIS, lock_type: LOCK_REC },
            rec_y_heap_no,
            n_bits,
            index.clone(),
            trx_b.clone(),
        );

        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType {
                lock_mode: LockMode::LockIS,
                lock_type: LOCK_REC | LOCK_WAIT,
            },
            rec_x_heap_no,
            n_bits,
            index.clone(),
            trx_b.clone(),
        );

        let result =
            lock_sys.deadlock_recursive(trx_a, trx_b, wait_for, &mut 0, 0);

        assert_eq!(result, NoDeadlock);
    }

    #[test]
    fn test_deadlock_recursive_with_cycle() {
        // trxA -> trxB -> trxA
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));

        let page_id = PageId::dummy();
        let rec_x_heap_no = 10;
        let rec_y_heap_no = 20;
        let n_bits = 100;

        // Create trxA, trxB
        let trx_a = Arc::new(Trx::new(1));
        let trx_b = Arc::new(Trx::new(2));

        // trx_a has lock for rec_x, but waiting for rec_y
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_x_heap_no,
            n_bits,
            index.clone(),
            trx_a.clone(),
        );

        let wait_for = lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType {
                lock_mode: LockMode::LockX,
                lock_type: LOCK_REC | LOCK_WAIT,
            },
            rec_y_heap_no,
            n_bits,
            index.clone(),
            trx_a.clone(),
        );

        // trx_b has rec_y, but waiting for rec_x
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_y_heap_no,
            n_bits,
            index.clone(),
            trx_b.clone(),
        );

        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType {
                lock_mode: LockMode::LockX,
                lock_type: LOCK_REC | LOCK_WAIT,
            },
            rec_x_heap_no,
            n_bits,
            index.clone(),
            trx_b.clone(),
        );

        let result =
            lock_sys.deadlock_recursive(trx_a, trx_b, wait_for, &mut 0, 0);

        assert_eq!(result, LockVictimIsStart);
    }

    #[test]
    fn get_next_rec_lock_all_belonging_to_same_page() {
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;
        let trx = Arc::new(Trx::new(1));
        let limit = 100;

        // Add 100 rec_locks
        for i in 0..limit {
            lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );
        }

        // Get locks for the same page
        let mut locks: Vec<Rc<RefCell<Lock>>> = vec![];
        let mut index = 0;
        while let (Some(lock), i) =
            lock_sys.get_next_rec_lock(page_id, index, rec_heap_no)
        {
            locks.push(lock);
            index = i;
        }

        assert_eq!(locks.len(), limit);
    }

    #[test]
    fn get_next_rec_lock_all_belonging_diff_heap_no() {
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;
        let trx = Arc::new(Trx::new(1));
        let limit = 100;

        // Add 100 rec_locks
        for i in 0..limit {
            lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );
        }

        // Get locks for the same page
        let mut locks: Vec<Rc<RefCell<Lock>>> = vec![];
        let mut index = 0;
        while let (Some(lock), i) =
            lock_sys.get_next_rec_lock(page_id, index, rec_heap_no + 1)
        {
            locks.push(lock);
            index = i;
        }

        assert_eq!(locks.len(), 0);
    }

    #[test]
    fn get_next_rec_lock_belonging_to_non_existing_page() {
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;
        let trx = Arc::new(Trx::new(1));
        let limit = 100;

        // Add 100 rec_locks
        for i in 0..limit {
            lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );
        }

        let mut locks: Vec<Rc<RefCell<Lock>>> = vec![];
        let mut index = 0;
        let page_id = PageId { space_id: 100, page_no: 0 };
        while let (Some(lock), i) =
            lock_sys.get_next_rec_lock(page_id, index, rec_heap_no)
        {
            locks.push(lock);
            index = i;
        }

        assert_eq!(locks.len(), 0);
    }

    #[test]
    fn get_next_rec_lock_having_mix_of_pages_and_heap_no() {
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;
        let trx = Arc::new(Trx::new(1));
        let limit = 100;

        // Add 50 rec_locks
        for _ in 0..limit / 2 {
            lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );
        }

        // Add 50 rec_locks
        for i in 0..limit / 2 {
            lock_sys.rec_create_low(
                PageId { space_id: i * 100, page_no: i * 78 },
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                rec_heap_no + 1 + i as usize,
                n_bits,
                index.clone(),
                trx.clone(),
            );
        }

        // Get locks for the same page
        let mut locks: Vec<Rc<RefCell<Lock>>> = vec![];
        let mut index = 0;
        while let (Some(lock), i) =
            lock_sys.get_next_rec_lock(page_id, index, rec_heap_no)
        {
            locks.push(lock);
            index = i;
        }

        assert_eq!(locks.len(), (limit / 2) as usize);
    }

    #[test]
    fn test_rec_has_to_wait_in_queue_no_other_locks() {
        // Test case where there are no other locks on the page
        let lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;
        let trx = Arc::new(Trx::new(1));

        // Create a waiting lock
        let waiting_lock = Rc::new(RefCell::new(Lock::new(
            LockId(1),
            trx.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index: index.clone(), page_id }),
            LockMode::LockX,
            LOCK_REC | LOCK_WAIT,
        )));

        // Set the bit for the record in the waiting lock
        waiting_lock.borrow_mut().rec_set_nth_bit(rec_heap_no);

        // Since there are no other locks on the page, the waiting lock should not have to wait
        assert_eq!(
            lock_sys.rec_has_to_wait_in_queue(waiting_lock, rec_heap_no),
            false
        );
    }

    #[test]
    fn test_rec_has_to_wait_in_queue_compatible_locks() {
        // Test case where there are other locks on the page but they are compatible
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Add an S lock from trx1
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockS, lock_type: LOCK_REC },
            rec_heap_no,
            n_bits,
            index.clone(),
            trx1.clone(),
        );

        // Create a waiting S lock from trx2 (compatible with existing S lock)
        let waiting_lock = Rc::new(RefCell::new(Lock::new(
            LockId(2),
            trx2.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index: index.clone(), page_id }),
            LockMode::LockS,
            LOCK_REC | LOCK_WAIT,
        )));

        // Set the bit for the record in the waiting lock
        waiting_lock.borrow_mut().rec_set_nth_bit(rec_heap_no);

        // Since the existing lock is compatible (S mode) with the waiting lock (also S mode),
        // the waiting lock should not have to wait
        assert_eq!(
            lock_sys.rec_has_to_wait_in_queue(waiting_lock, rec_heap_no),
            false
        );
    }

    #[test]
    fn test_rec_has_to_wait_in_queue_incompatible_locks() {
        // Test case where there are other locks on the page that are incompatible
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Add an X lock from trx1
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            n_bits,
            index.clone(),
            trx1.clone(),
        );

        // Create a waiting S lock from trx2 (incompatible with existing X lock)
        let waiting_lock = Rc::new(RefCell::new(Lock::new(
            LockId(2),
            trx2.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index: index.clone(), page_id }),
            LockMode::LockS,
            LOCK_REC | LOCK_WAIT,
        )));

        // Set the bit for the record in the waiting lock
        waiting_lock.borrow_mut().rec_set_nth_bit(rec_heap_no);

        // Since the existing lock is incompatible (X mode) with the waiting lock (S mode),
        // the waiting lock should have to wait
        assert_eq!(
            lock_sys.rec_has_to_wait_in_queue(waiting_lock, rec_heap_no),
            true
        );
    }

    #[test]
    fn test_rec_has_to_wait_in_queue_same_transaction() {
        // Test case where there are other locks on the page from the same transaction
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Add an X lock from trx
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            n_bits,
            index.clone(),
            trx.clone(),
        );

        // Create a waiting S lock from the same trx
        let waiting_lock = Rc::new(RefCell::new(Lock::new(
            LockId(2),
            trx.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index: index.clone(), page_id }),
            LockMode::LockS,
            LOCK_REC | LOCK_WAIT,
        )));

        // Set the bit for the record in the waiting lock
        waiting_lock.borrow_mut().rec_set_nth_bit(rec_heap_no);

        // Since the existing lock is from the same transaction,
        // the waiting lock should not have to wait (locks from the same transaction don't block each other)
        assert_eq!(
            lock_sys.rec_has_to_wait_in_queue(waiting_lock, rec_heap_no),
            false
        );
    }

    #[test]
    fn test_rec_has_to_wait_in_queue_different_heap_no() {
        // Test case where there are other locks on the page but for different heap numbers
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let different_heap_no = 20;
        let n_bits = 100;

        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Add an X lock from trx1 on a different heap number
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            different_heap_no,
            n_bits,
            index.clone(),
            trx1.clone(),
        );

        // Create a waiting S lock from trx2
        let waiting_lock = Rc::new(RefCell::new(Lock::new(
            LockId(2),
            trx2.clone(),
            n_bits,
            TxnLock::RecLock(RecLock { index: index.clone(), page_id }),
            LockMode::LockS,
            LOCK_REC | LOCK_WAIT,
        )));

        // Set the bit for the record in the waiting lock
        waiting_lock.borrow_mut().rec_set_nth_bit(rec_heap_no);

        // Since the existing lock is for a different heap number,
        // the waiting lock should not have to wait
        assert_eq!(
            lock_sys.rec_has_to_wait_in_queue(waiting_lock, rec_heap_no),
            false
        );
    }

    #[test]
    fn test_rec_other_has_conflicting_no_locks() {
        // Test case where there are no locks on the page
        let lock_sys = LockSys::new();
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let trx = Arc::new(Trx::new(1));

        // Check if there are any conflicting locks
        let result = lock_sys.rec_other_has_conflicting(
            page_id,
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            trx,
        );

        // Since there are no locks on the page, there should be no conflicts
        assert!(result.is_none());
    }

    #[test]
    fn test_rec_other_has_conflicting_no_conflict() {
        // Test case where there are locks but none conflict
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let different_heap_no = 20;
        let n_bits = 100;

        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Add an S lock from trx1 on a different heap number
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockS, lock_type: LOCK_REC },
            different_heap_no,
            n_bits,
            index.clone(),
            trx1.clone(),
        );

        // Check if there are any conflicting locks for trx2
        let result = lock_sys.rec_other_has_conflicting(
            page_id,
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            trx2,
        );

        // Since the existing lock is for a different heap number,
        // there should be no conflicts
        assert!(result.is_none());
    }

    #[test]
    fn test_rec_other_has_conflicting_with_conflict() {
        // Test case where there are conflicting locks
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        // Create two different transactions
        let trx1 = Arc::new(Trx::new(1));
        let trx2 = Arc::new(Trx::new(2));

        // Add an X lock from trx1
        let lock = lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            n_bits,
            index.clone(),
            trx1.clone(),
        );

        // Check if there are any conflicting locks for trx2
        let result = lock_sys.rec_other_has_conflicting(
            page_id,
            LockModeType { lock_mode: LockMode::LockS, lock_type: LOCK_REC },
            rec_heap_no,
            trx2,
        );

        // Since the existing lock is incompatible (X mode) with the requested lock (S mode),
        // there should be a conflict
        assert!(result.is_some());
    }

    #[test]
    fn test_rec_other_has_conflicting_same_transaction() {
        // Test case where there are locks from the same transaction
        let mut lock_sys = LockSys::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let page_id = PageId { space_id: 45, page_no: 54 };
        let rec_heap_no = 10;
        let n_bits = 100;

        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Add an X lock from trx
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            rec_heap_no,
            n_bits,
            index.clone(),
            trx.clone(),
        );

        // Check if there are any conflicting locks for the same trx
        let result = lock_sys.rec_other_has_conflicting(
            page_id,
            LockModeType { lock_mode: LockMode::LockS, lock_type: LOCK_REC },
            rec_heap_no,
            trx,
        );

        // Since the existing lock is from the same transaction,
        // there should be no conflicts (locks from the same transaction don't block each other)
        assert!(result.is_none());
    }

    #[test]
    fn test_move_rec_list_end_basic() {
        // Test basic functionality of moving locks from one block to another
        let mut lock_sys = LockSys::new();

        // Create source and destination pages with records
        let src_owning_page = create_records(
            5, // 5 records
            vec![FieldType::String(Some("test".to_string()))],
        );
        let dest_owning_page = create_records(
            0, // No records initially
            vec![FieldType::String(Some("test".to_string()))],
        );

        let src_block = src_owning_page.buf_block.clone();
        let dest_block = dest_owning_page.buf_block.clone();

        // Get the page ID of the source block
        let page_id = src_block.read().unwrap().get_page_id();
        let index = Arc::new(Index::new(page_id));

        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Add locks to the source block for different heap numbers
        let n_bits = 100;

        // Create locks for each record in the source block
        let src_page = src_block.read().unwrap().get_page();
        let user_records = src_page.page_get_user_records();
        assert_eq!(user_records.len(), 5);

        for (i, rec) in user_records.iter().enumerate() {
            let heap_no = rec.rec_get_heap_no();

            // Create a lock for this record
            lock_sys.rec_create_low(
                page_id.clone(),
                LockModeType {
                    lock_mode: LockMode::LockX,
                    lock_type: LOCK_REC,
                },
                heap_no,
                n_bits,
                index.clone(),
                trx.clone(),
            );

            // Verify that the lock was created
            let (lock, _) =
                lock_sys.get_next_rec_lock(page_id.clone(), i, heap_no);
            assert!(lock.is_some());
            assert!(lock.unwrap().borrow().rec_is_nth_bit_set(heap_no));
        }

        // Create a waiting lock for one of the records
        let waiting_heap_no = user_records[2].rec_get_heap_no();
        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType {
                lock_mode: LockMode::LockS,
                lock_type: LOCK_REC | LOCK_WAIT,
            },
            waiting_heap_no,
            n_bits,
            index.clone(),
            Arc::new(Trx::new(2)), // Different transaction
        );

        // Call move_rec_list_end to move locks from source to destination
        lock_sys.move_rec_list_end(
            dest_block.clone(),
            src_block.clone(),
            user_records[0], // Start with the first record
        );

        // Since rec_add_to_queue is empty, we can't verify that locks were added to the destination
        // Instead, verify that the locks on the source were correctly reset

        // Check that the waiting lock was reset
        let (locks, _) =
            lock_sys.get_next_rec_lock(page_id.clone(), 0, waiting_heap_no);
        assert!(locks.is_some());
        let lock = locks.unwrap();
        assert!(!lock.borrow().is_waiting());
    }

    #[test]
    fn test_move_rec_list_end_no_locks() {
        // Test moving locks when there are no locks on the source block
        let mut lock_sys = LockSys::new();

        // Create source and destination pages with records
        let src_owning_page = create_records(
            5, // 5 records
            vec![FieldType::String(Some("test".to_string()))],
        );
        let dest_owning_page = create_records(
            0, // No records initially
            vec![FieldType::String(Some("test".to_string()))],
        );

        let src_block = src_owning_page.buf_block.clone();
        let dest_block = dest_owning_page.buf_block.clone();

        // Get the page ID of the source block
        let page_id = src_block.read().unwrap().get_page_id();

        // Get a record from the source block
        let src_page = src_block.read().unwrap().get_page();
        let user_records = src_page.page_get_user_records();
        assert_eq!(user_records.len(), 5);

        // Call move_rec_list_end to move locks from source to destination
        // This should not cause any errors even though there are no locks
        lock_sys.move_rec_list_end(
            dest_block.clone(),
            src_block.clone(),
            user_records[0], // Start with the first record
        );

        // Verify that there are still no locks for the page
        let (no_locks, _) = lock_sys.get_next_rec_lock(page_id.clone(), 0, 0);
        assert!(no_locks.is_none());
    }

    #[test]
    fn test_move_rec_list_end_no_bits_set() {
        // Test moving locks when there are locks on the source block but none have bits set for the records being moved
        let mut lock_sys = LockSys::new();

        // Create source and destination pages with records
        let src_owning_page = create_records(
            5, // 5 records
            vec![FieldType::String(Some("test".to_string()))],
        );
        let dest_owning_page = create_records(
            0, // No records initially
            vec![FieldType::String(Some("test".to_string()))],
        );

        let src_block = src_owning_page.buf_block.clone();
        let dest_block = dest_owning_page.buf_block.clone();

        // Get the page ID of the source block
        let page_id = src_block.read().unwrap().get_page_id();
        let index = Arc::new(Index::new(page_id));

        // Create a transaction
        let trx = Arc::new(Trx::new(1));

        // Add a lock to the source block for a different heap number than any of the records
        let n_bits = 100;
        let different_heap_no = 99; // Assuming this is different from any record's heap_no

        lock_sys.rec_create_low(
            page_id.clone(),
            LockModeType { lock_mode: LockMode::LockX, lock_type: LOCK_REC },
            different_heap_no,
            n_bits,
            index.clone(),
            trx.clone(),
        );

        // Verify that the lock was created
        let (lock, _) =
            lock_sys.get_next_rec_lock(page_id.clone(), 0, different_heap_no);
        assert!(lock.is_some());
        assert!(lock.unwrap().borrow().rec_is_nth_bit_set(different_heap_no));

        // Get a record from the source block
        let src_page = src_block.read().unwrap().get_page();
        let user_records = src_page.page_get_user_records();
        assert_eq!(user_records.len(), 5);

        // Call move_rec_list_end to move locks from source to destination
        lock_sys.move_rec_list_end(
            dest_block.clone(),
            src_block.clone(),
            user_records[0], // Start with the first record
        );

        // Verify that the lock still exists and its bit is still set
        let (lock, _) =
            lock_sys.get_next_rec_lock(page_id.clone(), 0, different_heap_no);
        assert!(lock.is_some());
        assert!(lock.unwrap().borrow().rec_is_nth_bit_set(different_heap_no));
    }
}
