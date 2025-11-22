#[cfg(test)]
mod tests {
    use crate::buf::buf0iface::BufPoolLike;
    use crate::innodb::innodb0types::PageId;
    use crate::mtr::mtr0types::FixMode;
    use crate::mtr::{
        mtr0mtr::Mtr,
        mtr0types::{MemoSlot, MtrLogMode, MtrState, OtherResource},
    };
    use crate::tests::test0utils;
    use std::sync::Arc;

    #[derive(Default)]
    struct FakePool {
        released: std::cell::RefCell<Vec<(PageId, FixMode)>>,
    }

    impl BufPoolLike for FakePool {
        fn release_page(&self, pid: PageId, mode: FixMode) {
            self.released.borrow_mut().push((pid, mode));
        }
    }

    #[test]
    fn test_start_and_state() {
        let mut mtr = Mtr::new();
        assert!(!mtr.is_active());
        mtr.start();
        assert_eq!(mtr.get_mtr_state(), MtrState::MtrActive);
        assert!(mtr.is_active());
    }

    #[test]
    fn test_log_mode_and_disable() {
        let mut mtr = Mtr::new();
        mtr.start();
        assert_eq!(mtr.get_log_mode(), MtrLogMode::MtrLogModeAll);
        mtr.disable_redo_logging();
        assert_eq!(mtr.get_log_mode(), MtrLogMode::MtrLogNone);
    }

    #[test]
    fn test_savepoints_and_rollback() {
        let mut mtr = Mtr::new();
        mtr.start();
        let sp1 = mtr.set_savepoint();
        assert_eq!(sp1, 0);
        let sp2 = mtr.set_savepoint();
        assert_eq!(sp2, 0);
        // rollback to sp2 (no-op but should not panic)
        mtr.rollback_to_savepoint(sp2);
    }

    #[test]
    fn test_read_ulint_and_uint64() {
        let mut mtr = Mtr::new();
        mtr.start();
        let buf = [0x01u8, 0x02, 0x03, 0x04];
        assert_eq!(mtr.read_ulint(&buf, 4), 0x01020304);
        let buf64 = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(mtr.read_uint64(&buf64), 0x0102030405060708);
    }

    #[test]
    fn test_log_record_count() {
        let mut mtr = Mtr::new();
        assert_eq!(mtr.get_n_log_records(), 0);
        mtr.inc_n_log_records();
        assert_eq!(mtr.get_n_log_records(), 1);
    }

    #[test]
    fn test_memo_release_basic_removal() {
        let mut mtr = Mtr::new();
        mtr.start();

        let slot = MemoSlot::Other(OtherResource { id: 42, flags: 7 });

        mtr.memo_push(slot);

        assert_eq!(mtr.memo().len(), 1);

        mtr.memo_release(&slot);

        // slot should be removed
        assert_eq!(mtr.memo().len(), 0);
    }

    #[test]
    fn test_memo_release_removes_last_match() {
        let mut mtr = Mtr::new();
        mtr.start();

        let slot1 = MemoSlot::Other(OtherResource { id: 1, flags: 0 });
        let slot2 = MemoSlot::Other(OtherResource { id: 1, flags: 0 });

        mtr.memo_push(slot1);
        mtr.memo_push(slot2);

        mtr.memo_release(&slot1);

        // Should remove only the last occurrence
        assert_eq!(mtr.memo().len(), 1);
        assert_eq!(mtr.memo()[0], slot1); // the first one remains
    }

    #[test]
    #[should_panic(expected = "memo_release: slot not found")]
    fn test_memo_release_panic() {
        let mut mtr = Mtr::new();
        mtr.start();
        let slot = MemoSlot::Other(OtherResource { id: 1, flags: 0 });
        mtr.memo_release(&slot);
    }

    #[test]
    fn test_commit_resets_state() {
        let mut mtr = Mtr::new();
        mtr.start();
        mtr.memo_push(MemoSlot::Other(OtherResource { id: 3, flags: 0 }));
        mtr.commit();
        assert_eq!(mtr.get_mtr_state(), MtrState::MtrUndefined);
    }

    #[test]
    fn test_release_block_at_savepoint_basic() {
        //
        // 1. Create a fake page + block (test helper)
        //
        let owning_page = test0utils::create_raw_page();
        let block = owning_page.buf_block.clone();
        let pid = owning_page.page.page_get_page_id();

        //
        // 2. Set initial buf_fix_count = 1
        //
        {
            let mut g = block.write().unwrap();
            let page = g.buf_block_get_page_mut();
            page.set_buf_page_io_fix_count(1);
            assert_eq!(page.buf_page_fix_count(), 1);
        }

        //
        // 3. Create MTR and attach a *single shared* FakePool
        //
        let fake_pool = Arc::new(FakePool::default());
        let mut mtr = Mtr::new();
        mtr.start();
        mtr.attach_buf_pool(fake_pool.clone());

        //
        // 4. Set savepoint + memo push PageFix
        //
        let sp = mtr.set_savepoint();
        mtr.memo_push(MemoSlot::PageFix(pid, FixMode::Exclusive));

        assert_eq!(mtr.memo().len(), sp + 1);

        //
        // 5. Call release_block_at_savepoint
        //
        {
            // get mutable block reference
            let mut guard = block.write().unwrap();
            mtr.release_block_at_savepoint(sp, &mut guard);
            // guard dropped after this scope
        }

       

        //
        // 6. FakeBufPool must record release
        //
        {
            let releases = fake_pool.released.borrow();
            assert_eq!(releases.len(), 1);
            assert_eq!(releases[0], (pid, FixMode::Exclusive));
        }

        //
        // 7. Memo slot must be replaced with dummy
        //
        match &mtr.memo()[sp] {
            MemoSlot::Other(_) => {} // expected
            _ => panic!("memo slot at savepoint not replaced with dummy"),
        }
    }
}
