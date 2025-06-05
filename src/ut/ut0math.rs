/// Rounds up a number to the next power of 2.
///
/// This macro takes an integer value and rounds it up to the next power of 2.
/// It uses a bit manipulation algorithm that sets all bits to the right of the
/// most significant bit, then adds 1 to get the next power of 2.
///
/// # Examples
///
/// ```
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
