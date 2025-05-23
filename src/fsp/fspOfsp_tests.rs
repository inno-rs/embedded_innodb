#[cfg(test)]
mod tests {
    use crate::fil::fil0types::{
        FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN, FIL_PAGE_OFFSET,
        FIL_PAGE_SPACE_ID,
    };
    use crate::fsp::fspOfsp::*;
    use crate::fut::fut0lst::*;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;

    #[test]
    fn test_init_file_page_low_zero_and_headers() {
        let mut buf = vec![0xA5u8; UNIV_PAGE_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);
        let page_no: u32 = 42;
        let space_id: u32 = 7;

        desc.init_file_page_low(page_no, space_id);

        // Page‐number (4 bytes, big‐endian)
        assert_eq!(
            &buf[FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4],
            &page_no.to_be_bytes()
        );

        // LSN field (bytes FIL_PAGE_LSN .. FIL_PAGE_SPACE_ID) must be zero
        assert!(buf[FIL_PAGE_LSN..FIL_PAGE_SPACE_ID]
            .iter()
            .all(|&b| b == 0));

        // Space‐ID field
        assert_eq!(
            &buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4],
            &space_id.to_be_bytes()
        );

        // End‐LSN/checksum trailer
        let trailer_start = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
        assert!(buf[trailer_start..trailer_start + FIL_PAGE_END_LSN_CHKSUM]
            .iter()
            .all(|&b| b == 0));

        // Everything else should also be zero
        for (i, &byte) in buf.iter().enumerate() {
            let in_page_no   = (FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4).contains(&i);
            let in_lsn       = (FIL_PAGE_LSN..FIL_PAGE_SPACE_ID).contains(&i);
            let in_space_id  = (FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4).contains(&i);
            let in_trailer   = i >= trailer_start;
            if in_page_no || in_lsn || in_space_id || in_trailer {
                continue;
            }
            assert_eq!(
                byte, 0,
                "byte at offset {} should be zero, found 0x{:02x}", i, byte
            );
        }
    }

    #[test]
    fn test_xdes_init() {
        let mut buf = vec![0xAAu8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);
        desc.xdes_init();

        // state == XDES_FREE_STATE
        let mut state_bytes = [0u8; 4];
        state_bytes.copy_from_slice(&buf[XDES_STATE..XDES_STATE + 4]);
        assert_eq!(u32::from_le_bytes(state_bytes) as usize, XDES_FREE);

        // bitmap bytes all 0xFF
        assert!(buf[XDES_BITMAP..XDES_SIZE]
            .iter()
            .all(|&b| b == 0xFF));

        // other bytes untouched
        for (i, &b) in buf.iter().enumerate() {
            let in_state  = (XDES_STATE..XDES_STATE + 4).contains(&i);
            let in_bitmap = (XDES_BITMAP..XDES_SIZE).contains(&i);
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
        let mut buf = vec![0u8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);

        // Free‐bit on page 5
        assert!(!desc.get_bit(5, XDES_FREE_BIT));
        desc.set_bit(5, XDES_FREE_BIT, true);
        assert!(desc.get_bit(5, XDES_FREE_BIT));
        desc.set_bit(5, XDES_FREE_BIT, false);
        assert!(!desc.get_bit(5, XDES_FREE_BIT));

        // Clean‐bit on page 7
        assert!(!desc.get_bit(7, XDES_CLEAN_BIT));
        desc.set_bit(7, XDES_CLEAN_BIT, true);
        assert!(desc.get_bit(7, XDES_CLEAN_BIT));
    }

    #[test]
    fn test_used_count_and_find_bit() {
        let mut buf = vec![0u8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);

        // all free after init
        desc.xdes_init();
        assert_eq!(desc.used_count(), 0);
        assert_eq!(desc.find_bit(XDES_FREE_BIT), Some(0));

        // mark page 0 used
        desc.set_bit(0, XDES_FREE_BIT, false);
        assert_eq!(desc.used_count(), 1);
        assert_eq!(desc.find_bit(XDES_FREE_BIT), Some(1));

        // mark all remaining used
        for i in 1..FSP_EXTENT_SIZE {
            desc.set_bit(i, XDES_FREE_BIT, false);
        }
        assert_eq!(desc.used_count(), FSP_EXTENT_SIZE);
        assert_eq!(desc.find_bit(XDES_FREE_BIT), None);
    }

    #[test]
    fn test_alloc_page_basic_and_state() {
        let mut buf = vec![0u8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);
        desc.xdes_init();

        let extent_no = 5;
        let first = desc.alloc_page(extent_no)
            .expect("should get a page");
        assert_eq!(first, extent_no * FSP_EXTENT_SIZE as u32);

        // after one alloc, descriptor must be fragmented
        assert_eq!(desc.state(), XDES_FREE_FRAG);
    }

    #[test]
    fn test_alloc_page_exhaustion_and_full_state() {
        let mut buf = vec![0u8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);
        desc.xdes_init();

        let extent_no = 7;
        // allocate all 64 pages
        for i in 0..FSP_EXTENT_SIZE {
            let pg = desc.alloc_page(extent_no)
                .unwrap_or_else(|| panic!("alloc #{} failed", i));
            assert_eq!(pg, extent_no * FSP_EXTENT_SIZE as u32 + i as u32);

            // until the last, state remains FREE_FRAG
            if i < FSP_EXTENT_SIZE - 1 {
                assert_eq!(desc.state(), XDES_FREE_FRAG);
            } else {
                // after last, state flips to FULL_FRAG
                assert_eq!(desc.state(), XDES_FULL_FRAG);
            }
        }
        // now full: next alloc → None
        assert!(desc.alloc_page(extent_no).is_none());
    }

    #[test]

    fn test_header_init_sets_all_fields() {
        // Prepare a dummy 16 KiB page filled with non-zero to catch untouch regions
        let mut frame = vec![0xFFu8; UNIV_PAGE_SIZE];
        let mut desc = PageFrame::new(&mut frame);
        let sid = 1234;
        let sz  = 5678;

        // Invoke header_init
        desc.header_init(sid, sz);

        // 1) space_id written correctly
        let span = FSP_HEADER_OFFSET + FSP_SPACE_ID;
        assert_eq!(
            &frame[span .. span + 4],
            &sid.to_le_bytes(),
            "space_id mismatch"
        );

        // 2) space_size
        let span = FSP_HEADER_OFFSET + FSP_SIZE;
        assert_eq!(
            &frame[span .. span + 4],
            &sz.to_le_bytes(),
            "space_size mismatch"
        );

        // 3) free_limit = space_size
        let span = FSP_HEADER_OFFSET + FSP_FREE_LIMIT;
        assert_eq!(
            &frame[span .. span + 4],
            &sz.to_le_bytes(),
            "free_limit mismatch"
        );

        // 4) flags and frag_n_used remain zero
        let fspan = FSP_HEADER_OFFSET + FSP_SPACE_FLAGS;
        assert!(frame[fspan .. fspan + 4].iter().all(|&b| b == 0));
        let fspan = FSP_HEADER_OFFSET + FSP_FRAG_N_USED;
        assert!(frame[fspan .. fspan + 4].iter().all(|&b| b == 0));

        // 5) each of the five list-heads is initialized empty
        let lists = [
            FSP_FREE,
            FSP_FREE_FRAG,
            FSP_FULL_FRAG,
            FSP_SEG_INODES_FULL,
            FSP_SEG_INODES_FREE,
        ];
        for &rel_off in &lists {
            let abs_off = FSP_HEADER_OFFSET + rel_off;
            let slice  = &frame[abs_off .. abs_off + FLST_BASE_NODE_SIZE];

            // length == 0
            assert_eq!(
                FilAddr::len(slice),
                0,
                "list at rel_off {} length not zero",
                rel_off
            );

            // first == NULL, last == NULL
            assert_eq!(
                FilAddr::first(slice),
                FilAddr::NULL,
                "list at rel_off {} first not NULL",
                rel_off
            );
            assert_eq!(
                FilAddr::last(slice),
                FilAddr::NULL,
                "list at rel_off {} last not NULL",
                rel_off
            );
        }

        // 6) next_segment_id == 0
        let span = FSP_HEADER_OFFSET + FSP_SEG_ID;
        assert_eq!(
            &frame[span .. span + 4],
            &0u32.to_le_bytes(),
            "next_segment_id not zero"
        );
    }
}


