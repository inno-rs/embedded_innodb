use crate::ut::ut0dbg::ut_ad;

/// Store 1 byte in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
///
/// Store 1 byte (no endianness issue).
pub fn mach_write_to_1(buf: &mut [u8], val: u8) {
    ut_ad!(buf.len() >= 1);
    buf[0] = val;
}

/// Store 2 bytes, most‐significant byte first (big-endian).
pub fn mach_write_to_2(buf: &mut [u8], val: u16) {
    ut_ad!(buf.len() >= 2);
    buf[0] = (val >> 8) as u8;
    buf[1] = val as u8;
}

/// Store 3 bytes, MSB first (big-endian).
pub fn mach_write_to_3(buf: &mut [u8], val: u32) {
    ut_ad!(buf.len() >= 3);
    buf[0] = (val >> 16) as u8;
    buf[1] = (val >>  8) as u8;
    buf[2] =  val        as u8;
}

/// Store 4 bytes, MSB first (big-endian).
pub fn mach_write_to_4(buf: &mut [u8], val: u32) {
    ut_ad!(buf.len() >= 4);
    buf[0] = (val >> 24) as u8;
    buf[1] = (val >> 16) as u8;
    buf[2] = (val >>  8) as u8;
    buf[3] =  val        as u8;
}

/// Store 8 bytes, MSB first (big-endian) by two 4-byte calls.
pub fn mach_write_to_8(buf: &mut [u8], val: u64) {
    ut_ad!(buf.len() >= 8);
    let high = (val >> 32) as u32;
    let low  = val as u32;
    mach_write_to_4(&mut buf[0..4], high);
    mach_write_to_4(&mut buf[4..8], low);
}


/// Read 1 byte (no endianness issue).
pub fn mach_read_from_1(buf: &[u8]) -> u8 {
    ut_ad!(buf.len() >= 1);
    buf[0]
}

/// Read 2 bytes, most-significant byte first (big-endian).
pub fn mach_read_from_2(buf: &[u8]) -> u16 {
    ut_ad!(buf.len() >= 2);
    ((buf[0] as u16) << 8)
        |  (buf[1] as u16)
}

/// Read 3 bytes, MSB first (big-endian).
pub fn mach_read_from_3(buf: &[u8]) -> u32 {
    ut_ad!(buf.len() >= 3);
    ((buf[0] as u32) << 16)
        | ((buf[1] as u32) <<  8)
        |  (buf[2] as u32)
}

/// Read 4 bytes, MSB first (big-endian).
pub fn mach_read_from_4(buf: &[u8]) -> u32 {
    ut_ad!(buf.len() >= 4);
    ((buf[0] as u32) << 24)
        | ((buf[1] as u32) << 16)
        | ((buf[2] as u32) <<  8)
        |  (buf[3] as u32)
}

/// Read 8 bytes, MSB first (big-endian) by two 4-byte calls.
pub fn mach_read_from_8(buf: &[u8]) -> u64 {
    ut_ad!(buf.len() >= 8);
    let high = mach_read_from_4(&buf[0..4]) as u64;
    let low  = mach_read_from_4(&buf[4..8]) as u64;
    (high << 32) | low
}
/// Encode `v` in InnoDB's compressed-ULINT varint format into `buf`.
/// Returns the number of bytes written (1..5).
///
/// Format:
/// - If v <   2^7:   one byte 0xxxxxxx
/// - If v <   2^14:  two bytes 10xxxxxx xxxxxxxx
/// - If v <   2^21:  three bytes 110xxxxx xxxxxxxx xxxxxxxx
/// - If v <   2^28:  four bytes 1110xxxx xxxxxxxx xxxxxxxx xxxxxxxx
/// - If v >= 2^28:   five bytes 11110000 + [4-byte LE value]
///
/// Also supports "extended" patterns for values near the top of each range,
/// matching InnoDB's mach_write_compressed logic.
pub fn mach_write_compressed(buf: &mut [u8], v: u32) -> usize {
    // 1-byte (7-bit)
    if v < 0x80 {
        mach_write_to_1(&mut buf[0..1], v as u8);
        return 1;
    }
    // 2-byte (14-bit)
    if v < 0x4000 {
        mach_write_to_2(&mut buf[0..2], (v | 0x8000) as u16);
        return 2;
    }
    // 3-byte (21-bit)
    if v < 0x200000 {
        let val = (v | 0xC00000) as usize;
        mach_write_to_3(&mut buf[0..3], val as u32);
        return 3;
    }
    // 4-byte (28-bit)
    if v < 0x10000000 {
        mach_write_to_4(&mut buf[0..4], (v | 0xE0000000) as u32);
        return 4;
    }
    // extended 2-byte
    if v >= 0xFFFF_FC00 {
        mach_write_to_2(&mut buf[0..2], ((v & 0x3FF) | 0xF800) as u16);
        return 2;
    }
    // extended 3-byte
    if v >= 0xFFFE_0000 {
        let val = ((v & 0x1FFFF) | 0xFC0000) as usize;
        mach_write_to_3(&mut buf[0..3], val as u32);
        return 3;
    }
    // extended 4-byte
    if v >= 0xFF00_0000 {
        mach_write_to_4(&mut buf[0..4], ((v & 0xFFFFFF) | 0xFE000000) as u32);
        return 4;
    }
    // 5-byte fully expanded
    mach_write_to_1(&mut buf[0..1], 0xF0);
    mach_write_to_4(&mut buf[1..5], v);
    5
}

/// Compresses and writes a 64-bit integer into `buf`.
///
/// First writes the upper 32 bits (`v >> 32`) as a compressed ULINT
/// (1–5 bytes), then writes the lower 32 bits (`v as u32`) in 4 bytes LE.
///
/// Returns the total number of bytes written (compressed high part + 4).
pub fn mach_uint64_write_compressed(buf: &mut [u8], v: u64) -> usize {
    // 1) high 32 bits → compressed ULINT
    let high = (v >> 32) as u32;
    let n_high = mach_write_compressed(buf, high);

    // 2) low 32 bits → 4 bytes little-endian
    let low_be = (v as u32).to_be_bytes();
    buf[n_high..n_high + 4].copy_from_slice(&low_be);

    // total bytes = compressed_high + 4
    n_high + 4
}

/// Read 4 bytes from the buffer.
///
/// # Arguments
///
/// * `slice` - The slice to read from.
/// * `range` - The range of bytes to read (should be 4 bytes).
///
/// # Returns
///
/// * The value read as a `u32`.
#[macro_export]
macro_rules! mach_read_from_4 {
    ($slice:expr, $range:expr) => {{
        ut_ad!($range.end - $range.start == 4, "Expected range of 4 bytes");
        let bytes = &$slice[$range];
        ut_ad!(bytes.len() == 4, "Expected 4 bytes in the range");
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }};
}


/// Parse a compressed ULINT (32-bit) from the start of the buffer, advancing the pointer.
/// Mirrors C++ `mach_parse_compressed(byte*& ptr, const byte* end_ptr)`.
/// Returns `Some((value, new_ptr_index))` on success, or `None` if incomplete.
/// Parse exactly the same format, returning the decoded value + #bytes consumed.
pub fn mach_parse_compressed(buf: &[u8]) -> Option<(u32, usize)> {
    let first = *buf.get(0)?;
    // 1-byte
    if first < 0x80 {
        return Some((first as u32, 1));
    }
    // 2-byte
    if first < 0xC0 {
        let raw = u16::from_be_bytes(buf.get(0..2)?.try_into().unwrap());
        return Some(((raw & 0x3FFF) as u32, 2));
    }
    // 3-byte
    if first < 0xE0 {
        let raw = ((buf[0] as u32) << 16)
            | ((buf[1] as u32) <<  8)
            |  (buf[2] as u32);
        return Some((raw & 0x1F_FFFF, 3));
    }
    // 4-byte
    if first < 0xF0 {
        let raw = u32::from_be_bytes(buf.get(0..4)?.try_into().unwrap());
        return Some((raw & 0x0FFF_FFFF, 4));
    }
    // 5-byte full
    if first == 0xF0 {
        let low = u32::from_be_bytes(buf.get(1..5)?.try_into().unwrap());
        return Some((low, 5));
    }
    // extended-2: 0xF8..0xFB
    if (0xF8..0xFC).contains(&first) {
        let raw = u16::from_be_bytes(buf.get(0..2)?.try_into().unwrap());
        return Some((((raw & 0x03FF) as u32) | 0xFFFF_FC00, 2));
    }
    // extended-3: 0xFC..0xFD
    if (0xFC..0xFE).contains(&first) {
        let raw = ((buf[0] as u32) << 16)
            | ((buf[1] as u32) <<  8)
            |  (buf[2] as u32);
        return Some((raw | 0xFFFE_0000, 3));
    }
    // extended-4: 0xFE
    if first == 0xFE {
        let raw = ((buf[1] as u32) << 16)
            | ((buf[2] as u32) <<  8)
            |  (buf[3] as u32);
        return Some((raw | 0xFF00_0000, 4));
    }
    None
}
/// Parse a compressed 64-bit integer: first a compressed high‐32 bits, then 4 raw bytes.
/// Mirrors C++ `mach_uint64_parse_compressed(byte* ptr, byte* end_ptr, uint64_t* v)`.
/// Returns `Some((value, total_bytes_consumed))` or `None` on underflow.
pub fn mach_uint64_parse_compressed(buf: &[u8]) -> Option<(u64, usize)> {
    // need at least 5 bytes for high varint + 4-byte low
    if buf.len() < 5 {
        return None;
    }
    // parse high 32-bit varint
    let (high, used_high) = mach_parse_compressed(buf)?;
    let remaining = &buf[used_high..];
    if remaining.len() < 4 {
        return None;
    }
    let low = u32::from_be_bytes([
        remaining[0], remaining[1], remaining[2], remaining[3]
    ]) as u64;
    let value = ((high as u64) << 32) | low;
    Some((value, used_high + 4))
}

