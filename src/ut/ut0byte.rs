#![allow(dead_code)]

use crate::ut::ut0dbg::ut_ad;

/// Align `n` down to the nearest multiple of `align` (power-of-two).
#[inline]
pub fn align_down(n: usize, align: usize) -> usize {
    ut_ad!(align.is_power_of_two() && align > 0);
    n & !(align - 1)
}

/// Align `n` up to the nearest multiple of `align` (power-of-two).
#[inline]
pub fn align_up(n: usize, align: usize) -> usize {
    ut_ad!(align.is_power_of_two() && align > 0);
    let mask = align - 1;
    // Equivalent to (n + mask) & !mask; using branch avoids overflow concerns.
    if (n & mask) == 0 { n } else { (n & !mask).saturating_add(align) }
}

/// Align `n` (u64) down to the nearest multiple of `align` (power-of-two).
#[inline]
pub fn align_down_u64(n: u64, align: usize) -> u64 {
    let a = align as u64;
    ut_ad!(a.is_power_of_two() && a > 0);
    n & !(a - 1)
}

/// Align `n` (u64) up to the nearest multiple of `align` (power-of-two).
#[inline]
pub fn align_up_u64(n: u64, align: usize) -> u64 {
    let a = align as u64;
    ut_ad!(a.is_power_of_two() && a > 0);
    let mask = a - 1;
    // InnoDB uses (n + mask) & ~mask; we can avoid overflow with saturating_add.
    n.saturating_add(mask) & !mask
}

/// Return `ptr` rounded up to `align` (power-of-two).
#[inline]
pub fn align_ptr(ptr: *const u8, align: usize) -> *mut u8 {
    ut_ad!(align.is_power_of_two() && align > 0);
    let addr = ptr as usize;
    let aligned = align_up(addr, align);
    aligned as *mut u8
}

/// Return `ptr` rounded down to `align` (power-of-two).
#[inline]
pub fn align_ptr_down(ptr: *const u8, align: usize) -> *mut u8 {
    ut_ad!(align.is_power_of_two() && align > 0);
    let addr = ptr as usize;
    let aligned = align_down(addr, align);
    aligned as *mut u8
}

/// Offset of `ptr` from the previous `align` boundary (i.e., ptr % align).
#[inline]
pub fn align_offset(ptr: *const u8, align: usize) -> usize {
    ut_ad!(align.is_power_of_two() && align > 0);
    (ptr as usize) & (align - 1)
}

/// Get the n-th bit of `a` (0-based).
#[inline]
pub fn bit_get_nth(a: usize, n: usize) -> bool {
    ut_ad!(n < usize::BITS as usize);
    ((a >> n) & 1) != 0
}

/// Set/clear the n-th bit of `a` (0-based), returning the new value.
#[inline]
pub fn bit_set_nth(a: usize, n: usize, val: bool) -> usize {
    ut_ad!(n < usize::BITS as usize);
    if val { a | (1usize << n) } else { a & !(1usize << n) }
}
