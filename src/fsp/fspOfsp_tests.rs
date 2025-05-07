#[cfg(test)]
mod tests {

    use crate::fsp::fspOfsp::*;

    #[test]
    fn test_init_file_page_low_zero_and_headers() {
        // Using Rust Vec Macro. Making a slice of 16 KB where each entry is 165 (0xA5u8)
        let mut buf = vec![0xA5u8; UNIV_PAGE_SIZE];
        let page_no: u32 = 42;
        let space_id: u32 = 7;
        init_file_page_low(&mut buf, page_no, space_id);

        // Verify page number at offset
        assert_eq!(&buf[FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4], &page_no.to_be_bytes());

        // Verify first half of LSN area is zero
        assert!(buf[FIL_PAGE_LSN..FIL_PAGE_SPACE_ID].iter().all(|&b| b == 0));

        // Verify space ID bytes (overlaps second half of LSN region)
        assert_eq!(&buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4], &space_id.to_be_bytes());

        // Verify end-LSN/checksum trailer is zero
        let trailer_start = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
        assert!(buf[trailer_start..trailer_start + FIL_PAGE_END_LSN_CHKSUM]
            .iter()
            .all(|&b| b == 0));

        // Verify that all other bytes outside these regions are zero
        for (i, &byte) in buf.iter().enumerate() {
            let in_page_no = i >= FIL_PAGE_OFFSET && i < FIL_PAGE_OFFSET + 4;
            let in_lsn_start = i >= FIL_PAGE_LSN && i < FIL_PAGE_SPACE_ID;
            let in_space_id = i >= FIL_PAGE_SPACE_ID && i < FIL_PAGE_SPACE_ID + 4;
            let in_trailer = i >= trailer_start;
            if in_page_no || in_lsn_start || in_space_id || in_trailer {
                continue;
            }
            assert_eq!(byte, 0, "byte at offset {} should be zero, found 0x{:02x}", i, byte);
        }
    }

}