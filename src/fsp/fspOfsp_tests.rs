#[cfg(test)]
mod tests {
    use crate::fil::fil0types::{
        FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN, FIL_PAGE_OFFSET,
        FIL_PAGE_SPACE_ID,
    };
    use crate::fsp::fspOfsp::*;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;

    #[test]
    fn test_init_file_page_low_zero_and_headers() {
        // Using Rust Vec Macro. Making a slice of 16 KB where each entry is 165 (0xA5u8)
        let mut buf = vec![0xA5u8; UNIV_PAGE_SIZE];
        let page_no: u32 = 42;
        let space_id: u32 = 7;
        init_file_page_low(&mut buf, page_no, space_id);

        // Verify page number at offset
        assert_eq!(
            &buf[FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4],
            &page_no.to_be_bytes()
        );

        // Verify first half of LSN area is zero
        assert!(buf[FIL_PAGE_LSN..FIL_PAGE_SPACE_ID].iter().all(|&b| b == 0));

        // Verify space ID bytes (overlaps second half of LSN region)
        assert_eq!(
            &buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4],
            &space_id.to_be_bytes()
        );

        // Verify end-LSN/checksum trailer is zero
        let trailer_start = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
        assert!(
            buf[trailer_start..trailer_start + FIL_PAGE_END_LSN_CHKSUM]
                .iter()
                .all(|&b| b == 0)
        );

        // Verify that all other bytes outside these regions are zero
        for (i, &byte) in buf.iter().enumerate() {
            let in_page_no = i >= FIL_PAGE_OFFSET && i < FIL_PAGE_OFFSET + 4;
            let in_lsn_start = i >= FIL_PAGE_LSN && i < FIL_PAGE_SPACE_ID;
            let in_space_id =
                i >= FIL_PAGE_SPACE_ID && i < FIL_PAGE_SPACE_ID + 4;
            let in_trailer = i >= trailer_start;
            if in_page_no || in_lsn_start || in_space_id || in_trailer {
                continue;
            }
            assert_eq!(
                byte, 0,
                "byte at offset {} should be zero, found 0x{:02x}",
                i, byte
            );
        }
    }

    #[test]
    fn test_xdes_init() {
        let mut descr = vec![0xAAu8; XDES_SIZE];
        xdes_init(&mut descr);

        let expected_state = (XDES_FREE as u32).to_le_bytes();
        assert_eq!(
            &descr[XDES_STATE..XDES_STATE + 4],
            &expected_state,
            "state field incorrect"
        );

        assert!(
            descr[XDES_BITMAP..XDES_SIZE].iter().all(|&b| b == 0xFF),
            "bitmap not fully initialized to 0xFF"
        );

        // 5) Ensure bytes outside state+bitmap are untouched (still 0xAA)
        for (i, &b) in descr.iter().enumerate() {
            let in_state = i >= XDES_STATE && i < XDES_STATE + 4;
            let in_bitmap = i >= XDES_BITMAP && i < XDES_SIZE;
            if in_state || in_bitmap {
                continue;
            }
            assert_eq!(
                b, 0xAA,
                "byte at offset {} changed unexpectedly (was {:02x})",
                i, b
            );
        }
    }

    #[test]
    fn test_xdes_get_set_bit() {
        let mut descr = vec![0u8; XDES_SIZE];
        // Free bit operations at page 5
        assert!(!xdes_get_bit(&descr, 5, XDES_FREE_BIT));
        xdes_set_bit(&mut descr, 5, XDES_FREE_BIT, true);
        assert!(xdes_get_bit(&descr, 5, XDES_FREE_BIT));
        xdes_set_bit(&mut descr, 5, XDES_FREE_BIT, false);
        assert!(!xdes_get_bit(&descr, 5, XDES_FREE_BIT));
        // Clean bit operations at page 7
        assert!(!xdes_get_bit(&descr, 7, XDES_CLEAN_BIT));
        xdes_set_bit(&mut descr, 7, XDES_CLEAN_BIT, true);
        assert!(xdes_get_bit(&descr, 7, XDES_CLEAN_BIT));
    }

    #[test]
    fn test_xdes_get_n_used_and_find_bit() {
        #[test]
        fn test_xdes_get_n_used_and_find_bit() {
            let mut descr = vec![0u8; XDES_SIZE];
            // Initialize all as free
            xdes_init(&mut descr);
            // No pages are used
            assert_eq!(xdes_get_n_used(&descr), 0);
            // First free page is index 0
            assert_eq!(xdes_find_bit(&descr, XDES_FREE_BIT), Some(0));

            // Mark page 0 as used
            xdes_set_bit(&mut descr, 0, XDES_FREE_BIT, false);
            // Now one used page
            assert_eq!(xdes_get_n_used(&descr), 1);
            // Next free page is index 1
            assert_eq!(xdes_find_bit(&descr, XDES_FREE_BIT), Some(1));

            // If all pages marked used, find_bit returns None
            for idx in 1..FSP_EXTENT_SIZE {
                xdes_set_bit(&mut descr, idx, XDES_FREE_BIT, false);
            }
            assert_eq!(xdes_get_n_used(&descr), FSP_EXTENT_SIZE);
            assert_eq!(xdes_find_bit(&descr, XDES_FREE_BIT), None);
        }
    }
}
