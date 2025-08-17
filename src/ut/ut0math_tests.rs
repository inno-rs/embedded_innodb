#[cfg(test)]
mod tests {
    use crate::{
        innodb::innodb0types::IB_FILE_BLOCK_SIZE,
        ut::ut0math::{ut_2pow_remainder, ut_align_down, ut_is_2pow},
        ut_2_power_up,
    };

    #[test]
    fn test_ut_2_power_up() {
        assert_eq!(ut_2_power_up!(0), 1);
        assert_eq!(ut_2_power_up!(1), 1);
        assert_eq!(ut_2_power_up!(2), 2);
        assert_eq!(ut_2_power_up!(3), 4);
        assert_eq!(ut_2_power_up!(4), 4);
        assert_eq!(ut_2_power_up!(5), 8);
        assert_eq!(ut_2_power_up!(6), 8);
        assert_eq!(ut_2_power_up!(7), 8);
        assert_eq!(ut_2_power_up!(8), 8);
        assert_eq!(ut_2_power_up!(9), 16);
        assert_eq!(ut_2_power_up!(1023), 1024);
        assert_eq!(ut_2_power_up!(1024), 1024);
        assert_eq!(ut_2_power_up!(1025), 2048);
        assert_eq!(ut_2_power_up!(u32::MAX), 0x1_0000_0000);
    }

    #[test]
    fn test_ut_is_2pow() {
        assert!(!ut_is_2pow(0));
        assert!(ut_is_2pow(1));
        assert!(!ut_is_2pow(12));
        assert!(ut_is_2pow(64));
        assert!(ut_is_2pow(256));
        assert!(!ut_is_2pow(255));
    }

    #[test]
    fn test_ut_align_down() {
        assert_eq!(ut_align_down(10, IB_FILE_BLOCK_SIZE), 0);
        assert_eq!(ut_align_down(100, IB_FILE_BLOCK_SIZE), 0);
        assert_eq!(ut_align_down(1000, IB_FILE_BLOCK_SIZE), 512);
        assert_eq!(ut_align_down(512 * 2 + 1, IB_FILE_BLOCK_SIZE), 512 * 2);
        assert_eq!(ut_align_down(9787, IB_FILE_BLOCK_SIZE), 9728);
    }

    #[test]
    fn test_ut_2pow_remainder() {
        // Test with power of 2 divisors
        assert_eq!(ut_2pow_remainder(10, 2), 0);
        assert_eq!(ut_2pow_remainder(11, 2), 1);
        assert_eq!(ut_2pow_remainder(12, 4), 0);
        assert_eq!(ut_2pow_remainder(13, 4), 1);
        assert_eq!(ut_2pow_remainder(14, 4), 2);
        assert_eq!(ut_2pow_remainder(15, 4), 3);
        assert_eq!(ut_2pow_remainder(16, 4), 0);

        // Test with larger power of 2 divisors
        assert_eq!(ut_2pow_remainder(100, 8), 4);
        assert_eq!(ut_2pow_remainder(1000, 16), 8);
        assert_eq!(ut_2pow_remainder(1023, 1024), 1023);
        assert_eq!(ut_2pow_remainder(1024, 1024), 0);
        assert_eq!(ut_2pow_remainder(1025, 1024), 1);

        // Test with IB_FILE_BLOCK_SIZE (which should be 512, a power of 2)
        assert_eq!(ut_2pow_remainder(512, IB_FILE_BLOCK_SIZE), 0);
        assert_eq!(ut_2pow_remainder(513, IB_FILE_BLOCK_SIZE), 1);
        assert_eq!(ut_2pow_remainder(1023, IB_FILE_BLOCK_SIZE), 511);
        assert_eq!(ut_2pow_remainder(1024, IB_FILE_BLOCK_SIZE), 0);
    }
}
