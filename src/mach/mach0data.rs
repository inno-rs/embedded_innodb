/// Store 1 byte in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_1(buf: &mut [u8], val: u8) {
    buf[0] = val ;
}

/// Store two bytes in big-endian order (MSB first).
///
/// # Arguments
///
/// * `buf` - The 2-byte buffer to write into.
/// * `val` - The value to write (must fit in 16 bits).
pub fn mach_write_to_2(buf: &mut [u8], val: u16) {
    buf[0] = ((val >> 8) & 0xFF) as u8; // high byte
    buf[1] = (val & 0xFF) as u8;        // low byte
}
/// Store 4 bytes in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_4(buf: &mut [u8], val: u32) {
    buf[0] = ((val >> 24) & 0xFF) as u8;
    buf[1] = ((val >> 16) & 0xFF) as u8;
    buf[2] = ((val >> 8) & 0xFF) as u8;
    buf[3] = (val & 0xFF) as u8;
}
/// Write eight bytes big-endian into `buf` by reusing the 4-byte helper.
pub fn mach_write_to_8(buf: &mut [u8], val: u64) {
    let high = (val >> 32) as u32;
    let low  = val as u32;
    // Write the high 32 bits first
    mach_write_to_4(&mut buf[0..4], high);
    // Then the low 32 bits
    mach_write_to_4(&mut buf[4..8], low);
}

/// Write a 3-byte little-endian integer into `buf[0..3]` from `val`.
/// Similar to mach_write_to_N helpers for N=3.
fn mach_write_to_3(buf: &mut [u8], val: u32) {
    // Most significant byte (bits 16..23)
    buf[0] = ((val >> 16) & 0xFF) as u8;
    // Next byte (bits 8..15)
    buf[1] = ((val >> 8) & 0xFF) as u8;
    // Least significant byte (bits 0..7)
    buf[2] = (val & 0xFF) as u8;
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
    buf[0] = 0xF0;
    let tail = v.to_be_bytes();       // [u8; 4]
    buf[1..5].copy_from_slice(&tail);
    5
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
