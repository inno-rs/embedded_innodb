#[cfg(test)]
mod tests {
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
        let v = 0x1234u32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 2);
        // Now little-endian: buf[0] = low byte, buf[1] = high|0x80
        let combined = u16::from_le_bytes([buf[0], buf[1]]);
        // Lower 14 bits should equal v
        assert_eq!(combined & 0x3FFF, v as u16);
        // High‐bit (0x8000) should be set
        assert_eq!(combined & 0x8000, 0x8000);
    }

    #[test]
    fn test_compress_3_byte_standard() {
        let v = 0x1F_FFFFu32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 3);
        // Prefix bits are now in the third byte (little-endian)
        assert_eq!(buf[2] & 0xE0, 0xC0);

        // And the remaining 21 bits reconstruct to v:
        let mut tmp = [0u8; 4];
        tmp[..3].copy_from_slice(&buf[..3]); // low→high
        let combined = u32::from_le_bytes(tmp) & 0x1F_FFFF;
        assert_eq!(combined, v);
    }

    #[test]
    fn test_compress_4_byte_standard() {
        let v = 0x0F_FFFF_FFu32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 4);
        // Prefix bits in the 4th byte
        assert_eq!(buf[3] & 0xF0, 0xE0);

        // And the remaining 28 bits reconstruct to v:
        let mut tmp = [0u8; 4];
        tmp.copy_from_slice(&buf[..4]);
        let combined = u32::from_le_bytes(tmp) & 0x0FFF_FFFF;
        assert_eq!(combined, v);
    }

    #[test]
    fn test_compress_5_byte_full() {
        let v = 0x1234_5678u32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 5);
        assert_eq!(buf[0], 0xF0);
        // Tail is little-endian
        let tail = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(tail, v);
    }
}
