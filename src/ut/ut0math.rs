use std::{
    fmt::Debug,
    ops::{BitOr, Shl, Shr},
};

use crate::ut::ut0dbg::ut_ad;

/// Rounds up a number to the next power of 2.
///
/// This macro takes an integer value and rounds it up to the next power of 2.
/// It uses a bit manipulation algorithm that sets all bits to the right of the
/// most significant bit, then adds 1 to get the next power of 2.
///
/// # Examples
///
/// ```
/// use innodb::ut_2_power_up;
/// assert_eq!(ut_2_power_up!(0), 1);     // Special case: 0 returns 1
/// assert_eq!(ut_2_power_up!(1), 1);     // Special case: 1 returns 1
/// assert_eq!(ut_2_power_up!(3), 4);     // 3 rounds up to 4 (2^2)
/// assert_eq!(ut_2_power_up!(4), 4);     // 4 is already a power of 2, returns 4
/// assert_eq!(ut_2_power_up!(5), 8);     // 5 rounds up to 8 (2^3)
/// assert_eq!(ut_2_power_up!(1023), 1024); // 1023 rounds up to 1024 (2^10)
/// ```
///
/// # Parameters
/// * `$x` - The value to round up to the next power of 2
///
/// # Returns
/// * The next power of 2 greater than or equal to the input value
/// * Returns 1 for inputs 0 and 1
#[macro_export]
macro_rules! ut_2_power_up {
    ($x:expr) => {{
        let mut x = $x as u64;
        if x <= 1 {
            1
        } else {
            x -= 1;
            x |= x >> 1;
            x |= x >> 2;
            x |= x >> 4;
            x |= x >> 8;
            x |= x >> 16;
            x |= x >> 32;
            x + 1
        }
    }};
}

/// A trait for unsigned integer types that can be used with the math utility functions.
///
/// This trait is implemented for all types that satisfy the requirements for
/// bitwise operations and comparisons needed by functions like `ut_is_2pow` and
/// `ut_align_down`.
///
/// # Examples
///
/// ```
/// // The trait is automatically implemented for standard unsigned integer types
/// use innodb::ut::ut0math::{UInt, ut_is_2pow};
///
/// fn check_power_of_two<T: UInt>(value: T) -> bool {
///     ut_is_2pow(value)
/// }
///
/// assert!(check_power_of_two(64u32));
/// assert!(check_power_of_two(256u64));
/// ```
pub trait UInt:
    Copy
    + Debug
    + PartialEq
    + PartialOrd
    + From<u8>
    + std::ops::BitAnd<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Add<Output = Self>
    + std::ops::Not<Output = Self>
    + Shr<Self, Output = Self>
    + Shl<Self, Output = Self>
    + BitOr<Output = Self>
{
    const BITS: u32;
}

impl<T> UInt for T
where
    T: Copy
        + Debug
        + PartialEq
        + PartialOrd
        + From<u8>
        + std::ops::BitAnd<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Add<Output = Self>
        + std::ops::Not<Output = T>
        + Shr<Self, Output = Self>
        + Shl<Self, Output = Self>
        + BitOr<Output = Self>,
{
    const BITS: u32 = size_of::<T>() as u32 * 8;
}

/// Checks if the provided number is a power of 2.
///
/// This function uses a bitwise algorithm to determine if a number is a power of 2.
/// A number is a power of 2 if and only if it has exactly one bit set to 1.
/// The algorithm works by checking if the number is non-zero and if the bitwise AND
/// of the number and (number - 1) is zero.
///
/// # Examples
///
/// ```
/// use innodb::ut::ut0math::ut_is_2pow;
///
/// assert!(!ut_is_2pow(0));     // 0 is not a power of 2
/// assert!(ut_is_2pow(1));      // 1 is 2^0
/// assert!(ut_is_2pow(2));      // 2 is 2^1
/// assert!(!ut_is_2pow(3));     // 3 is not a power of 2
/// assert!(ut_is_2pow(4));      // 4 is 2^2
/// assert!(ut_is_2pow(64));     // 64 is 2^6
/// assert!(!ut_is_2pow(100));   // 100 is not a power of 2
/// ```
///
/// # Parameters
/// * `n` - The number to check
///
/// # Returns
/// * `true` if the number is a power of 2
/// * `false` if the number is 0 or not a power of 2
pub fn ut_is_2pow<T: UInt>(n: T) -> bool {
    n != T::from(0) && (n & (n - T::from(1))) == T::from(0)
}

/// Aligns a number down to the nearest multiple of the given alignment.
///
/// This function rounds down a number to the nearest multiple of the alignment value.
/// The alignment value must be a power of 2. The function works by masking out the
/// lower bits of the number that represent the offset from the alignment boundary.
///
/// # Examples
///
/// ```
/// use innodb::ut::ut0math::ut_align_down;
///
/// // Align to 512 (IB_FILE_BLOCK_SIZE)
/// assert_eq!(ut_align_down(10, 512), 0);
/// assert_eq!(ut_align_down(512, 512), 512);
/// assert_eq!(ut_align_down(513, 512), 512);
/// assert_eq!(ut_align_down(1000, 512), 512);
/// assert_eq!(ut_align_down(1024, 512), 1024);
/// assert_eq!(ut_align_down(1025, 512), 1024);
///
/// // Align to powers of 2
/// assert_eq!(ut_align_down(15, 8), 8);
/// assert_eq!(ut_align_down(16, 8), 16);
/// assert_eq!(ut_align_down(17, 8), 16);
/// ```
///
/// # Parameters
/// * `n` - The number to align down
/// * `align_no` - The alignment value (must be a power of 2)
///
/// # Returns
/// * The largest multiple of `align_no` that is less than or equal to `n`
///
/// # Panics
/// * In debug mode, panics if `align_no` is 0 or not a power of 2
pub fn ut_align_down<T: UInt>(n: T, align_no: T) -> T {
    ut_ad!(align_no > T::from(0));
    ut_ad!(ut_is_2pow(align_no));

    n & !(align_no - T::from(1))
}

/// Aligns a number up to the nearest multiple of the given alignment.
///
/// This function rounds up a number to the nearest multiple of the alignment value.
/// The alignment value must be a power of 2.
/// The function works by masking out the
/// lower bits of the number that represent the offset from the alignment boundary.
///
/// # Examples
///
/// ```
/// use innodb::ut::ut0math::ut_align_up;
///
/// // Align to 512 (IB_FILE_BLOCK_SIZE)
/// assert_eq!(ut_align_up(10, 512), 512);
/// assert_eq!(ut_align_up(512, 512), 512);
/// assert_eq!(ut_align_up(513, 512), 1024);
/// assert_eq!(ut_align_up(1000, 512), 1024);
/// assert_eq!(ut_align_up(1024, 512), 1024);
/// assert_eq!(ut_align_up(1025, 512), 1536);
///
/// // Align to powers of 2
/// assert_eq!(ut_align_up(15, 8), 16);
/// assert_eq!(ut_align_up(16, 8), 16);
/// assert_eq!(ut_align_up(17, 8), 24);
/// ```
///
/// # Parameters
/// * `n` - The number to align down
/// * `align_no` - The alignment value (must be a power of 2)
///
/// # Returns
/// * The largest multiple of `align_no` that is less than or equal to `n`
///
/// # Panics
/// * In debug mode, panics if `align_no` is 0 or not a power of 2
pub fn ut_align_up<T: UInt>(n: T, align_no: T) -> T {
    ut_ad!(align_no > T::from(0));
    ut_ad!(ut_is_2pow(align_no));

    ut_align_down(n + align_no - T::from(1), align_no)
}

/// Calculates a fast remainder of n/m when m is power of two.
///
/// This function uses a bitwise AND operation to compute the remainder,
/// which is more efficient than the standard modulus operation.
///
/// # Arguments
///
/// * `n` - The dividend.
/// * `m` - The divisor, which must be a power of two.
///
/// # Returns
///
/// * The remainder of n divided by m.
pub fn ut_2pow_remainder<T: UInt>(n: T, m: T) -> T {
    ut_ad!(m > T::from(0));
    ut_ad!(ut_is_2pow(m));

    n & (m - T::from(1))
}
