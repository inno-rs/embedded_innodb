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
