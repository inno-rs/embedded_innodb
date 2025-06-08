#[cfg(test)]
mod tests {
    use crate::mtr::mtr0mtr::Mtr;
    use crate::mtr::mtr0types::{MemoSlot, MtrLogMode, MtrState, OtherResource};
    use super::*;

    #[test]
    fn test_start_and_state() {
        let mut mtr = Mtr::new();
        assert!(!mtr.is_active());
        mtr.start();
        assert_eq!(mtr.get_mtr_state(), MtrState::MTR_ACTIVE);
        assert!(mtr.is_active());
    }

    #[test]
    fn test_log_mode_and_disable() {
        let mut mtr = Mtr::new();
        mtr.start();
        assert_eq!(mtr.get_log_mode(), MtrLogMode::MTR_LOG_MODE_ALL);
        mtr.disable_redo_logging();
        assert_eq!(mtr.get_log_mode(), MtrLogMode::MTR_LOG_NONE);
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
        let mtr = Mtr::new();
        let buf = [0x01, 0x02, 0x03];
        assert_eq!(mtr.read_ulint(&buf, 3), 0x030201);
        let buf64 = [1u8,2,3,4,5,6,7,8];
        assert_eq!(mtr.read_uint64(&buf64), 0x0807060504030201);
    }

    #[test]
    fn test_log_record_count() {
        let mut mtr = Mtr::new();
        assert_eq!(mtr.get_n_log_records(), 0);
        mtr.inc_n_log_records();
        assert_eq!(mtr.get_n_log_records(), 1);
    }

    #[test]
    fn test_memo_release() {
        let mut mtr = Mtr::new();
        mtr.start();
        let slot = MemoSlot::Other(OtherResource { id: 42, flags: 7 });
        mtr.memo_push(slot.clone());
        // release should succeed
        mtr.memo_release(&slot);
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
        assert_eq!(mtr.get_mtr_state(), MtrState::MTR_UNDEFINED);
    }
}
