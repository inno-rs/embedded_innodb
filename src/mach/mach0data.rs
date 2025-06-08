/// Store 1 byte in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_1(buf: &mut [u8], val: usize) {
    buf[0] = val as u8;
}

/// Store 2 bytes in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_2(buf: &mut [u8], val: usize) {
    buf.copy_from_slice(&(val as u16).to_le_bytes());
}

/// Store 4 bytes in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_4(buf: &mut [u8], val: usize) {
    buf.copy_from_slice(&(val as u32).to_le_bytes());
}

/// Store 8 bytes in the buffer.
///
/// # Arguments
///
/// * `buf` - The buffer to write to.
/// * `val` - The value to write.
///
pub fn mach_write_to_8(buf: &mut [u8], val: usize) {
    buf.copy_from_slice(&(val as u64).to_le_bytes());
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
