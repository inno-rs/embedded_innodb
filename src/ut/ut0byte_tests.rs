#[cfg(test)]
mod tests {
    use crate::ut::{
        ut0byte::{bit_get_nth, bit_set_nth},
        ut0math::UInt,
    };

    fn test_round_trip<T: UInt>(mut a: T, bit: u32) {
        let bit_pos = T::from(bit as u8);
        let mask = T::from(1) << bit_pos;

        // Set bit
        a = bit_set_nth(a, bit_pos, true);
        assert!(bit_get_nth(a, bit_pos));
        assert_eq!(a & mask, mask);

        // Clear bit
        a = bit_set_nth(a, bit_pos, false);
        assert!(!bit_get_nth(a, bit_pos));
        assert_eq!(a & mask, T::from(0));
    }

    #[test]
    fn test_bit_operations_u8() {
        test_round_trip(0u8, 0);
        test_round_trip(0u8, 7);
        test_round_trip(0b1010_1010u8, 3);
    }

    #[test]
    fn test_bit_operations_u16() {
        test_round_trip(0u16, 0);
        test_round_trip(0u16, 15);
        test_round_trip(0b1010_1010_1010_1010u16, 12);
    }

    #[test]
    fn test_bit_operations_u32() {
        test_round_trip(0u32, 0);
        test_round_trip(0u32, 31);
        test_round_trip(0x8000_0000u32, 31);
    }

    #[test]
    fn test_bit_operations_u64() {
        test_round_trip(0u64, 0);
        test_round_trip(0u64, 63);
        test_round_trip(0x8000_0000_0000_0000u64, 63);
    }

    #[test]
    fn test_bit_operations_u128() {
        test_round_trip(0u128, 0);
        test_round_trip(0u128, 127);
        test_round_trip(0x8000_0000_0000_0000_0000_0000_0000_0000u128, 127);
    }

    #[test]
    fn test_bit_set_and_get_multiple_bits() {
        let mut x = 0u8;
        x = bit_set_nth(x, 1u8, true);
        x = bit_set_nth(x, 5u8, true);
        assert!(bit_get_nth(x, 1u8));
        assert!(bit_get_nth(x, 5u8));
        assert!(!bit_get_nth(x, 0u8));

        x = bit_set_nth(x, 1u8, false);
        assert!(!bit_get_nth(x, 1u8));
    }
}
