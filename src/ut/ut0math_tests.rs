#[cfg(test)]
mod tests {
    use crate::ut_2_power_up;

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
}
