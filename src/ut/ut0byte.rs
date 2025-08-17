#![allow(dead_code)]

use crate::ut::{ut0dbg::ut_ad, ut0math::UInt};

/// Returns the value of the _n-th bit_ (0-based) in the given integer `a`.
///
/// # Type Parameters
/// - `T`: An unsigned integer type implementing the `UInt` trait.
///
/// # Arguments
/// - `a`: The integer from which to extract the bit.
/// - `n`: The 0-based bit index to read.
///
/// # Returns
/// - `true` if the `n`-th bit is `1`, `false` if it is `0`.
///
/// # Panics
/// This function panics in debug mode if `n >= T::BITS` via `ut_ad!`.
///
/// # Examples
/// ```
/// use innodb::ut::ut0byte::bit_get_nth;
/// let x = 0b0000_1000u8;
/// assert!(bit_get_nth(x, 3));
/// assert!(!bit_get_nth(x, 2));
/// ```
#[inline]
pub fn bit_get_nth<T: UInt>(a: T, n: T) -> bool {
    ut_ad!(n < T::from(T::BITS as u8));

    ((a >> n) & T::from(1)) != T::from(0)
}

/// Sets or clears the _n-th bit_ (0-based) of the given integer `a` depending on `val`,
/// and returns the updated value.
///
/// # Type Parameters
/// - `T`: An unsigned integer type implementing the `UInt` trait.
///
/// # Arguments
/// - `a`: The input value to modify.
/// - `n`: The 0-based bit index to modify.
/// - `val`: If `true`, the bit is set to `1`. If `false`, it is cleared to `0`.
///
/// # Returns
/// A new value with the `n`-th bit updated according to `val`.
///
/// # Panics
/// This function panics in debug mode if `n >= T::BITS` via `ut_ad!`.
///
/// # Examples
/// ```
/// use innodb::ut::ut0byte::bit_set_nth;
/// let x = 0b0000_0000u8;
/// let x = bit_set_nth(x, 2, true);
/// assert_eq!(x, 0b0000_0100u8);
///
/// let x = bit_set_nth(x, 2, false);
/// assert_eq!(x, 0b0000_0000u8);
/// ```
#[inline]
pub fn bit_set_nth<T: UInt>(a: T, n: T, val: bool) -> T {
    ut_ad!(n < T::from(T::BITS as u8));

    let mask = T::from(1) << n;
    if val { a | mask } else { a & !mask }
}
