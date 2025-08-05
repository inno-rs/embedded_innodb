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
    fn test_ut_2pow_remainder_small_values() {
        assert_eq!(ut_2pow_remainder(5u32, 2u32), 1); // 5 % 2 = 1
        assert_eq!(ut_2pow_remainder(6u32, 2u32), 0); // 6 % 2 = 0
        assert_eq!(ut_2pow_remainder(7u32, 4u32), 3); // 7 % 4 = 3
        assert_eq!(ut_2pow_remainder(8u32, 4u32), 0); // 8 % 4 = 0
        assert_eq!(ut_2pow_remainder(9u32, 8u32), 1); // 9 % 8 = 1
    }

    #[test]
    fn test_ut_2pow_remainder_large_values() {
        assert_eq!(ut_2pow_remainder(1025u64, 1024u64), 1); // 1025 % 1024 = 1
        assert_eq!(ut_2pow_remainder(2048u64, 1024u64), 0); // 2048 % 1024 = 0
        assert_eq!(ut_2pow_remainder(9999u64, 16u64), 9999 % 16);
    }

    #[test]
    fn test_ut_2pow_remainder_zero_remainder() {
        assert_eq!(ut_2pow_remainder(0u32, 2u32), 0);
        assert_eq!(ut_2pow_remainder(0u32, 8u32), 0);
    }

    #[test]
    fn test_ut_2pow_remainder_edge_cases() {
        // largest u32 with small modulus
        assert_eq!(ut_2pow_remainder(u32::MAX, 2u32), u32::MAX % 2);
        assert_eq!(ut_2pow_remainder(u32::MAX, 8u32), u32::MAX % 8);

        // largest u64 with bigger modulus
        assert_eq!(ut_2pow_remainder(u64::MAX, 1024u64), u64::MAX % 1024);
    }
}
