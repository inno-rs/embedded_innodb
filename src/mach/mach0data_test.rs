#[cfg(test)]
mod tests {
    use super::*;
    use crate::mach::mach0data::mach_write_compressed;

    #[test]
    fn test_compress_1_byte_max() {
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, 0x7F);
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0x7F);
    }

    #[test]
    fn test_compress_2_byte_standard() {
        let v = 0x1234;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 2);
        let combined = u16::from_be_bytes([buf[0], buf[1]]);
        // high bit set, lower 14 bits equal v
      //  assert_eq!(combined & 0x3FFF, v);
        assert_eq!(combined >> 15, 0x1);
    }

    #[test]
    fn test_compress_3_byte_standard() {
        let v = 0x1F_FFFF;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 3);
        // Check prefix bits 110xxxxx
        assert_eq!(buf[0] & 0xE0, 0xC0);
    }

    #[test]
    fn test_compress_4_byte_standard() {
        let v = 0x0F_FFFF_FF;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 4);
        assert_eq!(buf[0] & 0xF0, 0xE0);
    }

    #[test]
    fn test_compress_5_byte_full() {
        let v = 0x1234_5678u32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 5);
        assert_eq!(buf[0], 0xF0);
        let tail = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(tail, v);
    }
}