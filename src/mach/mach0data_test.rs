#[cfg(test)]
mod tests {
    use crate::mach::mach0data::{
        mach_parse_compressed, mach_read_from_1, mach_read_from_2,
        mach_read_from_3, mach_read_from_4, mach_read_from_8,
        mach_uint64_parse_compressed, mach_write_compressed, mach_write_to_1,
        mach_write_to_2, mach_write_to_3, mach_write_to_4, mach_write_to_8,
    };

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

        // Big-endian varint: [ high|0x80, low ]
        let combined_be = u16::from_be_bytes([buf[0], buf[1]]);
        // lower 14 bits == v
        assert_eq!(combined_be & 0x3FFF, v as u16);
        // high-bit set
        assert_eq!(combined_be & 0x8000, 0x8000);
    }

    #[test]
    fn test_compress_3_byte_standard() {
        let v = 0x1F_FFFFu32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 3);

        // high-order tag bits in buf[0]
        assert_eq!(buf[0] & 0xE0, 0xC0);

        // reconstruct big-endian raw, mask off the 5 tag bits:
        let raw_be =
            ((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32);
        assert_eq!(raw_be & 0x1F_FFFF, v);
    }

    #[test]
    fn test_compress_4_byte_standard() {
        let v = 0x0F_FFFF_FFu32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 4);

        // high-order tag bits in buf[0]
        assert_eq!(buf[0] & 0xF0, 0xE0);

        // reconstruct big-endian raw, mask off the 4 tag bits:
        let raw_be = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        assert_eq!(raw_be & 0x0FFF_FFFF, v);
    }

    #[test]
    fn test_compress_5_byte_full() {
        let v = 0x1234_5678u32;
        let mut buf = [0u8; 5];
        let n = mach_write_compressed(&mut buf, v);
        assert_eq!(n, 5);

        // first byte == 0xF0 tag
        assert_eq!(buf[0], 0xF0);

        // tail was written with mach_write_to_4 (big-endian)
        let tail_be = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(tail_be, v);
    }

    fn round_trip(v: u32) {
        // allocate max 5 bytes
        let mut buf = [0u8; 5];
        let written = mach_write_compressed(&mut buf, v);
        let (decoded, consumed) = mach_parse_compressed(&buf[..written])
            .expect("parse should succeed");
        assert_eq!(decoded, v, "value round-trip for 0x{:X}", v);
        assert_eq!(consumed, written, "length for 0x{:X}", v);
    }

    #[test]
    fn test_compress_and_parse_varints() {
        // 1-byte
        round_trip(0x7F);
        // 2-byte standard
        round_trip(0x1234);
        // 3-byte standard (max: 2^21-1)
        round_trip(0x1F_FFFF);
        // 4-byte standard (max: 2^28-1)
        round_trip(0x0FFF_FFFF);
        // full 5-byte fallback
        round_trip(0x1234_5678);
        // extended 2-byte (≥0xFFFFFC00)
        round_trip(0xFFFF_FC10);
        // extended 3-byte (0xFFFE0000..0xFFFFFC00)
        round_trip(0xFFFE_00AA);
        // extended 4-byte (0xFF000000..0xFFFE0000)
        round_trip(0xFF00_1234);
    }

    /// Helper: build a compressed-64 buffer for `v`, then parse it.

    fn round_trip_64(v: u64) {
        let high = (v >> 32) as u32;
        let low = v as u32;

        // 1) compress high-word
        let mut tmp = [0u8; 5];
        let n_hi = mach_write_compressed(&mut tmp, high);

        // 2) append low-word BE
        let mut buf = Vec::with_capacity(n_hi + 4);
        buf.extend_from_slice(&tmp[..n_hi]);
        buf.extend_from_slice(&low.to_be_bytes());

        // 3) parse it back
        let (decoded, consumed) = mach_uint64_parse_compressed(&buf)
            .expect(&format!("parse failed for 0x{:X}", v));

        assert_eq!(decoded, v, "decoded value mismatch for 0x{:X}", v);
        assert_eq!(consumed, n_hi + 4, "consumed length for 0x{:X}", v);
    }

    #[test]
    fn test_uint64_roundtrips() {
        // tiny high-word (1-byte varint)
        round_trip_64((0x7F_u64 << 32) | 0xDEADBEEFu64);
        // 2-byte normal
        round_trip_64((0x1234_u64 << 32) | 0xFEEDFACEu64);
        // 3-byte normal
        round_trip_64((0x1F_FFFF_u64 << 32) | 0xCAFEBABEu64);
        // 4-byte normal
        round_trip_64((0x0FFF_FFFF_u64 << 32) | 0x01234567u64);
        // 5-byte fallback (high≥2^28)
        round_trip_64((0x1234_5678_u64 << 32) | 0x89ABCDEFu64);
        // extended-2 (≥0xFFFF_FC00)
        round_trip_64((0xFFFF_FC10_u64 << 32) | 0x00000001u64);
        // extended-3 (0xFFFE_0000..)
        round_trip_64((0xFFFE_00AA_u64 << 32) | 0x00000002u64);
        // extended-4 (0xFF00_0000..)
        round_trip_64((0xFF00_1234_u64 << 32) | 0x00000003u64);
    }

    #[test]
    fn test_uint64_underflow() {
        // too short for even minimal header
        assert!(mach_uint64_parse_compressed(&[]).is_none());
        assert!(
            mach_uint64_parse_compressed(&[0x00, 0x01, 0x02, 0x03]).is_none()
        );
        // high varint OK but missing some low bytes
        let mut tmp = [0u8; 5];
        let n_hi = {
            // compress a small high-word so uses 1 byte
            mach_write_compressed(&mut tmp, 0x12)
        };
        let buf = &tmp[..n_hi]; // only high-word
        assert!(mach_uint64_parse_compressed(buf).is_none());
        // high + only 3 low‐bytes
        let mut buf2 = Vec::from(&tmp[..n_hi]);
        buf2.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        assert!(mach_uint64_parse_compressed(&buf2).is_none());
    }

    #[test]
    fn roundtrip_1_byte() {
        for &v in &[0x00_u8, 0x7F, 0xFF] {
            let mut buf = [0u8; 1];
            mach_write_to_1(&mut buf, v);
            let got = mach_read_from_1(&buf);
            assert_eq!(got, v, "1-byte roundtrip failed for 0x{:02X}", v);
        }
    }

    #[test]
    fn roundtrip_2_bytes() {
        for &v in &[0x0000_u16, 0x1234, 0xABCD, 0xFFFF] {
            let mut buf = [0u8; 2];
            mach_write_to_2(&mut buf, v);
            let got = mach_read_from_2(&buf);
            assert_eq!(got, v, "2-byte roundtrip failed for 0x{:04X}", v);
        }
    }

    #[test]
    fn roundtrip_3_bytes() {
        for &v in &[0x000000_u32, 0x00ABCD, 0x123456, 0xFFFFFF] {
            let mut buf = [0u8; 3];
            mach_write_to_3(&mut buf, v);
            let got = mach_read_from_3(&buf);
            // since write_to_3 only takes low 3 bytes, mask:
            assert_eq!(
                got & 0xFF_FFFF,
                v & 0xFF_FFFF,
                "3-byte roundtrip failed for 0x{:06X}",
                v
            );
        }
    }

    #[test]
    fn roundtrip_4_bytes() {
        for &v in &[0x00000000_u32, 0xDEADBEEF, 0xFFFFFFFF] {
            let mut buf = [0u8; 4];
            mach_write_to_4(&mut buf, v);
            let got = mach_read_from_4(&buf);
            assert_eq!(got, v, "4-byte roundtrip failed for 0x{:08X}", v);
        }
    }

    #[test]
    fn roundtrip_8_bytes() {
        for &v in &[
            0x00000000_00000000_u64,
            0x1122_3344_5566_7788,
            0xFFFF_FFFF_FFFF_FFFF,
        ] {
            let mut buf = [0u8; 8];
            mach_write_to_8(&mut buf, v);
            let got = mach_read_from_8(&buf);
            assert_eq!(got, v, "8-byte roundtrip failed for 0x{:016X}", v);
        }
    }
}