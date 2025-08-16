mod tests {

    #[test]
    fn test_align_down_up_usize() {
        use crate::ut::ut0byte::{align_down, align_up};
        assert_eq!(align_down(0, 4096), 0);
        assert_eq!(align_up(0, 4096), 0);
        assert_eq!(align_down(4096, 4096), 4096);
        assert_eq!(align_up(4096, 4096), 4096);
        assert_eq!(align_down(4097, 4096), 4096);
        assert_eq!(align_up(4097, 4096), 8192);
        assert_eq!(align_down(12345, 4096), 12288);
        assert_eq!(align_up(12345, 4096), 16384);
    }

    #[test]
    fn test_align_down_up_u64() {
        use crate::ut::ut0byte::{align_down_u64, align_up_u64};
        assert_eq!(align_down_u64(12345, 4096), 12288);
        assert_eq!(align_up_u64(12345, 4096), 16384);
    }

    #[test]
    fn test_ptr_align() {
        use crate::ut::ut0byte::{align_ptr, align_ptr_down};
        let buf = [0u8; 9000];
        let p = buf.as_ptr();
        let up = align_ptr(p, 4096) as usize;
        let down = align_ptr_down(unsafe { p.add(5000) }, 4096) as usize;
        assert_eq!(up % 4096, 0);
        assert_eq!(down % 4096, 0);
    }

    #[test]
    fn test_align_offset() {
        use crate::ut::ut0byte::align_offset;
        let base = 0x10000usize as *const u8;
        let p = unsafe { base.add(123) };
        assert_eq!(align_offset(p, 64), 123 & 63);
    }

    #[test]
    fn test_bits() {
        use crate::ut::ut0byte::{bit_get_nth, bit_set_nth};
        let mut x = 0usize;
        x = bit_set_nth(x, 5, true);
        assert!(bit_get_nth(x, 5));
        x = bit_set_nth(x, 5, false);
        assert!(!bit_get_nth(x, 5));
    }
}
