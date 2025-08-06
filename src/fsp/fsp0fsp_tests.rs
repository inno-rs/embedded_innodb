#[cfg(test)]
mod tests {
    use crate::{
        fil::fil0types::{
            FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN, FIL_PAGE_OFFSET,
            FIL_PAGE_SPACE_ID, FILL_NULL,
        },
        fsp::{fsp0fsp::*, fsp0types::FSegId},
        fut::fut0lst::*,
        innodb::innodb0types::{RawPage, UNIV_PAGE_SIZE},
        mtr::{
            mtr0mtr::Mtr,
            mtr0types::{FixMode, MemoSlot},
        },
        tests::test0utils::create_fseg_inode_page,
    };

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
        assert!(buf[FIL_PAGE_LSN..FIL_PAGE_SPACE_ID].iter().all(|&b| b == 0));

        // Space‐ID field
        assert_eq!(
            &buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4],
            &space_id.to_be_bytes()
        );

        // End‐LSN/checksum trailer
        let trailer_start = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
        assert!(
            buf[trailer_start..trailer_start + FIL_PAGE_END_LSN_CHKSUM]
                .iter()
                .all(|&b| b == 0)
        );

        // Everything else should also be zero
        for (i, &byte) in buf.iter().enumerate() {
            let in_page_no =
                (FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4).contains(&i);
            let in_lsn = (FIL_PAGE_LSN..FIL_PAGE_SPACE_ID).contains(&i);
            let in_space_id =
                (FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4).contains(&i);
            let in_trailer = i >= trailer_start;
            if in_page_no || in_lsn || in_space_id || in_trailer {
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
        let mut buf = vec![0xAAu8; XDES_SIZE];
        let mut desc = ExtentDescriptor::new(&mut buf);
        desc.xdes_init();

        // state == XDES_FREE_STATE
        let mut state_bytes = [0u8; 4];
        state_bytes.copy_from_slice(&buf[XDES_STATE..XDES_STATE + 4]);
        assert_eq!(u32::from_le_bytes(state_bytes) as usize, XDES_FREE);

        // bitmap bytes all 0xFF
        assert!(buf[XDES_BITMAP..XDES_SIZE].iter().all(|&b| b == 0xFF));

        // other bytes untouched
        for (i, &b) in buf.iter().enumerate() {
            let in_state = (XDES_STATE..XDES_STATE + 4).contains(&i);
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
        let first = desc.alloc_page(extent_no).expect("should get a page");
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
            let pg = desc
                .alloc_page(extent_no)
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
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        let sid = 1234;
        let sz = 5678;

        // Invoke header_init
        fsp_header_page.header_init(sid, sz);

        // 1) space_id written correctly
        let span = FSP_HEADER_OFFSET + FSP_SPACE_ID;
        assert_eq!(
            &frame[span..span + 4],
            &sid.to_le_bytes(),
            "space_id mismatch"
        );

        // 2) space_size
        let span = FSP_HEADER_OFFSET + FSP_SIZE;
        assert_eq!(
            &frame[span..span + 4],
            &sz.to_le_bytes(),
            "space_size mismatch"
        );

        // 3) free_limit = space_size
        let span = FSP_HEADER_OFFSET + FSP_FREE_LIMIT;
        assert_eq!(
            &frame[span..span + 4],
            &sz.to_le_bytes(),
            "free_limit mismatch"
        );

        // 4) flags and frag_n_used remain zero
        let fspan = FSP_HEADER_OFFSET + FSP_SPACE_FLAGS;
        assert!(frame[fspan..fspan + 4].iter().all(|&b| b == 0));
        let fspan = FSP_HEADER_OFFSET + FSP_FRAG_N_USED;
        assert!(frame[fspan..fspan + 4].iter().all(|&b| b == 0));

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
            let slice = &frame[abs_off..abs_off + FLST_BASE_NODE_SIZE];

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
            &frame[span..span + 4],
            &0u32.to_le_bytes(),
            "next_segment_id not zero"
        );
    }

    #[test]
    fn test_alloc_free_extent_moves_extent_correctly() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        // Step 1: Initialize header
        fsp_header_page.header_init(1, 1000);

        // Step 2: Prepare extent descriptor at offset 190
        let extent_off = 190;
        let index = (extent_off - XDES_ARR_OFFSET) / XDES_SIZE;
        assert_eq!(index, 1);

        // Step 3: Scope 1 - Initialize extent descriptor and link it into FSP_FREE
        {
            // Split to avoid overlapping borrows
            let buf = fsp_header_page.get_buf_mut();
            let (head, tail) = buf.split_at_mut(extent_off); // head: 0..extent_off, tail: extent_off..

            let xdes_buf = &mut tail[..XDES_SIZE]; // offset into tail = [extent_off..extent_off+40]

            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init();
            xdes.set_state(XDES_FREE);

            let me = FilAddr { page_no: 0, byte_off: extent_off as u16 };

            let base = &mut head[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            flst_init(base);
            me.add_last(base, xdes_buf, None);
        }

        // Step 4: Call alloc_free_extent
        let result = fsp_header_page.alloc_free_extent();

        // Step 5: Assertions
        assert_eq!(result, Some(1));

        // Step 6: Scope 2 - Verify FSP_FREE is empty
        {
            let buf = fsp_header_page.get_buf_mut();
            let base = &buf[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            assert_eq!(FilAddr::len(base), 0);
            assert_eq!(FilAddr::first(base), FilAddr::NULL);
            assert_eq!(FilAddr::last(base), FilAddr::NULL);
        }

        // Step 7: Scope 3 - Verify FSP_FREE_FRAG contains the extent
        {
            let buf = fsp_header_page.get_buf_mut();
            let base = &buf[FSP_HEADER_OFFSET + FSP_FREE_FRAG
                ..FSP_HEADER_OFFSET + FSP_FREE_FRAG + FLST_BASE_NODE_SIZE];
            let me = FilAddr { page_no: 0, byte_off: extent_off as u16 };
            assert_eq!(FilAddr::len(base), 1);
            assert_eq!(FilAddr::first(base), me);
            assert_eq!(FilAddr::last(base), me);
        }

        // Step 8: Scope 4 - Verify descriptor state and bitmap
        {
            let buf = fsp_header_page.get_buf_mut();
            let xdes_buf = &mut buf[extent_off..extent_off + XDES_SIZE];
            let xdes = ExtentDescriptor::new(xdes_buf);
            assert_eq!(xdes.state(), XDES_FREE_FRAG);
            assert_eq!(xdes.used_count(), 0); // All bits set = all pages free
        }
    }

    #[test]
    fn test_free_page_moves_extent_to_free_if_all_pages_freed() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(1, 1000);

        //Correctly aligned extent setup
        let extent_no = 0;
        let extent_off = XDES_ARR_OFFSET + extent_no * XDES_SIZE;
        let page_idx = 63;
        let page_no = (extent_no * FSP_EXTENT_SIZE + page_idx) as u32;

        {
            // Split to avoid borrow conflict
            let buf = fsp_header_page.get_buf_mut();
            let (head, tail) = buf.split_at_mut(extent_off);
            let xdes_buf = &mut tail[..XDES_SIZE];

            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init(); // all pages free
            xdes.set_bit(page_idx, XDES_FREE_BIT, false); // mark page 63 as used
            xdes.set_state(XDES_FREE_FRAG);

            let addr = FilAddr {
                page_no: (extent_no * FSP_EXTENT_SIZE) as u32,
                byte_off: extent_off as u16,
            };

            let free_frag = &mut head[FSP_HEADER_OFFSET + FSP_FREE_FRAG
                ..FSP_HEADER_OFFSET + FSP_FREE_FRAG + FLST_BASE_NODE_SIZE];
            flst_init(free_frag);
            addr.add_last(free_frag, xdes_buf, None);
        }

        // Now free that last used page
        fsp_header_page.free_page(page_no);

        {
            let buf = fsp_header_page.get_buf_mut();
            let xdes_buf = &mut buf[extent_off..extent_off + XDES_SIZE];
            let xdes = ExtentDescriptor::new(xdes_buf);

            assert_eq!(xdes.get_bit(page_idx, XDES_FREE_BIT), true);
            assert_eq!(xdes.used_count(), 0);
            assert_eq!(xdes.state(), XDES_FREE);
        }

        {
            let buf = fsp_header_page.get_buf_mut();
            let free = &buf[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            let free_frag = &buf[FSP_HEADER_OFFSET + FSP_FREE_FRAG
                ..FSP_HEADER_OFFSET + FSP_FREE_FRAG + FLST_BASE_NODE_SIZE];

            assert_eq!(FilAddr::len(free), 1);
            assert_eq!(FilAddr::len(free_frag), 0);
        }
    }

    #[test]
    fn test_alloc_page_from_free_frag_list() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        let space_id = 7;
        let space_size = 256;

        fsp_header_page.header_init(space_id, space_size);

        {
            let buf = fsp_header_page.get_buf_mut();
            let extent_off = XDES_ARR_OFFSET;

            // SAFELY split buffer
            let (head_part, xdes_area) = buf.split_at_mut(XDES_ARR_OFFSET);
            let rel_off = extent_off - XDES_ARR_OFFSET;
            let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];

            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init();
            xdes.set_state(XDES_FREE_FRAG);

            let this_addr =
                FilAddr { page_no: 0, byte_off: extent_off as u16 };

            let free_frag_head =
                &mut head_part[FSP_HEADER_OFFSET + FSP_FREE_FRAG..];
            this_addr.add_last(free_frag_head, xdes_buf, None);
        }

        let page_no = fsp_header_page.alloc_page();

        assert!(page_no.is_some());
        let page_no = page_no.unwrap();
        assert_eq!(page_no, 0);

        // Re-borrow xdes to verify post-state
        let buf = fsp_header_page.get_buf_mut();
        let (head_part, xdes_area) = buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = 0; // First extent
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let xdes = ExtentDescriptor::new(xdes_buf);

        // Check that page 0 was marked used
        assert!(!xdes.get_bit(0, XDES_FREE_BIT), "Page not marked used");

        // Should still be in FREE_FRAG since only 1 page used
        assert_eq!(xdes.state(), XDES_FREE_FRAG);
    }

    #[test]
    fn test_alloc_single_page_new_extent() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(99, 256); // space_id = 99, space size = 256 pages

        // Step 1: Setup extent descriptor and add it to FSP_FREE
        {
            let (header_part, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let extent_off = XDES_ARR_OFFSET;
            let rel_off = extent_off - XDES_ARR_OFFSET;
            let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];

            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init();
            xdes.set_state(XDES_FREE);

            let head = &mut header_part[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            let this_addr =
                FilAddr { page_no: 0, byte_off: extent_off as u16 };
            this_addr.add_last(head, xdes_buf, None);
        }

        // Step 2: Allocate single page
        let page_no = fsp_header_page.alloc_single_page();
        assert!(page_no.is_some());
        let page_no = page_no.unwrap();
        assert_eq!(page_no, 0); // First page of extent 0

        // Step 3: Check that bit is marked as used
        {
            let (_, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let rel_off = 0;
            let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
            let xdes = ExtentDescriptor::new(xdes_buf);
            assert_eq!(xdes.get_bit(0, XDES_FREE_BIT), false);
        }
    }

    #[test]
    fn test_alloc_n_pages_zero() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(99, 256);

        let result = fsp_header_page.alloc_n_pages(0);
        assert!(result.is_none());
    }

    #[test]
    fn test_alloc_n_pages_one() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(99, 256);

        // Setup a FREE extent at offset 0
        {
            let (header_part, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[0..XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init();
            xdes.set_state(XDES_FREE);

            let head = &mut header_part[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            let this_addr =
                FilAddr { page_no: 0, byte_off: XDES_ARR_OFFSET as u16 };
            this_addr.add_last(head, xdes_buf, None);
        }

        let result = fsp_header_page.alloc_n_pages(1);
        assert_eq!(result, Some(vec![0]));
    }

    #[test]
    fn test_alloc_n_pages_full_extent() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(99, 256);

        // Setup a FREE extent at offset 0
        {
            let (header_part, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[0..XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init();
            xdes.set_state(XDES_FREE);

            let head = &mut header_part[FSP_HEADER_OFFSET + FSP_FREE
                ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
            let this_addr =
                FilAddr { page_no: 0, byte_off: XDES_ARR_OFFSET as u16 };
            this_addr.add_last(head, xdes_buf, None);
        }

        let result = fsp_header_page.alloc_n_pages(FSP_EXTENT_SIZE);
        assert_eq!(result, Some((0..FSP_EXTENT_SIZE as u32).collect()));
    }

    #[test]
    fn test_alloc_n_pages_unsupported_size() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(99, 256);

        let result = fsp_header_page.alloc_n_pages(5);
        assert!(result.is_none());
    }

    #[test]
    fn test_extend_space_creates_new_extent() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        // Initialize header with 64 pages (1 extent)
        fsp_header_page.header_init(99, 64);

        // Call extend_space
        let new_extent = fsp_header_page.extend_space();
        assert_eq!(new_extent, Some(1)); // This should be extent #1

        // Verify that the space size was updated
        let new_size =
            fsp_header_page.read_u32_le(FSP_HEADER_OFFSET + FSP_SIZE);
        assert_eq!(new_size, 128); // 64 (old) + 64 (new extent)

        // Get extent descriptor at new extent offset
        let extent_off = XDES_ARR_OFFSET + 1 * XDES_SIZE;
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let (_, xdes_area) =
            fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let xdes = ExtentDescriptor::new(xdes_buf);

        // Check the extent is free
        assert_eq!(xdes.state(), XDES_FREE);
        for i in 0..FSP_EXTENT_SIZE {
            assert!(xdes.get_bit(i, XDES_FREE_BIT));
        }
    }

    #[test]
    fn test_is_page_free() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(0, 64); // Initialize with 1 extent (64 pages)

        // Initialize extent descriptor for extent 0
        {
            let (_, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[0..XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init(); // All pages free
            xdes.set_bit(0, XDES_FREE_BIT, false); // Mark page 0 as used
        }

        // Test page 0 (should be used)
        assert_eq!(fsp_header_page.is_page_free(0), false);

        // Test page 1 (should be free)
        assert_eq!(fsp_header_page.is_page_free(1), true);
    }

    #[test]
    fn test_is_extent_used() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(0, 64); // Initialize with 1 extent (64 pages)

        // Initialize extent descriptor for extent 0
        {
            let (_, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[0..XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init(); // All pages free
            xdes.set_bit(0, XDES_FREE_BIT, false); // Mark page 0 as used
        }

        // Test extent 0 (should be used)
        assert_eq!(fsp_header_page.is_extent_used(0), true);

        // Initialize extent descriptor for extent 1
        {
            let rel_off = XDES_SIZE; // Offset for extent 1
            let (_, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init(); // All pages free
        }

        // Test extent 1 (should be free)
        assert_eq!(fsp_header_page.is_extent_used(1), false);
    }

    #[test]
    fn test_space_available() {
        let mut raw_page = RawPage::new();
        let frame = raw_page.as_page_frame().0;
        let mut fsp_header_page = FspHeaderPage::new(frame);

        fsp_header_page.header_init(0, 64); // Initialize with 1 extent (64 pages)

        // Initialize extent descriptor for extent 0
        {
            let (_, xdes_area) =
                fsp_header_page.get_buf_mut().split_at_mut(XDES_ARR_OFFSET);
            let xdes_buf = &mut xdes_area[0..XDES_SIZE];
            let mut xdes = ExtentDescriptor::new(xdes_buf);
            xdes.xdes_init(); // All pages free
            xdes.set_bit(0, XDES_FREE_BIT, false); // Mark page 0 as used
        }

        // Test space_available (should be 63)
        assert_eq!(fsp_header_page.space_available(), 63);
    }

    #[test]
    fn test_fsp_seg_inode_page_new() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        let seg_ids =
            fsp_seg_inode_page.fsp_seg_inode_page_get_seg_ids(&mut mtr);

        for seg_id in seg_ids {
            assert_eq!(seg_id, FSegId(0u64), "Segment ID mismatch");
        }
    }

    #[test]
    fn test_fsp_seg_inode_page_get_nth_inode() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        // Write segment id
        let segment_id = 42u64;
        let mut fseg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_get_nth_inode(19, &mut mtr);

        fseg_inode.fseg_inode_set_fseg_id(FSegId(segment_id));

        // Verify segment id is set correctly
        let read_fseg_id = fsp_seg_inode_page
            .fsp_seg_inode_page_get_nth_inode(19, &mut mtr)
            .fseg_inode_get_fseg_id();

        assert_eq!(read_fseg_id, segment_id, "Segment ID mismatch");
    }

    #[test]
    fn test_fsp_seg_inode_page_find_free() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        // Write segment id to all except one.
        let skip_index = 19;
        for index in 0..FSP_SEG_INODES_PER_PAGE {
            if index == skip_index {
                continue;
            }

            let segment_id = 42u64;
            let mut fseg_inode = fsp_seg_inode_page
                .fsp_seg_inode_page_get_nth_inode(index, &mut mtr);

            fseg_inode.fseg_inode_set_fseg_id(FSegId(segment_id));
        }

        // Now find free inode
        let fseg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_find_free(&mut mtr);
        assert!(fseg_inode.is_some(), "Should find a free inode");
        let found_index = fseg_inode.unwrap();
        assert_eq!(found_index, skip_index, "Found wrong inode");
    }

    #[test]
    fn test_fsp_seg_inode_page_find_used() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        // Write segment id to all except one.
        let write_index = 19;

        let segment_id = 42u64;
        let mut fseg_inode = fsp_seg_inode_page
            .fsp_seg_inode_page_get_nth_inode(write_index, &mut mtr);

        fseg_inode.fseg_inode_set_fseg_id(FSegId(segment_id));

        // Now find free inode
        let fseg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_find_used(&mut mtr);
        assert!(fseg_inode.is_some(), "Should find a free inode");
        let found_index = fseg_inode.unwrap();
        assert_eq!(found_index, write_index, "Found wrong inode");
    }

    #[test]
    fn test_fseg_get_nth_frag_page_no() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        let mut fsp_seg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_get_nth_inode(0, &mut mtr);

        fsp_seg_inode.init(&mut mtr);

        // Test that all frag pages are set to FILL_NULL
        {
            let frag_pages = fsp_seg_inode.fseg_get_all_frag_pages();
            assert_eq!(frag_pages.len(), FSEG_FRAG_ARR_N_SLOTS);

            for frag_page_no in frag_pages {
                assert_eq!(frag_page_no, FILL_NULL);
            }
        }

        // Set some fragment page numbers
        {
            let test_pages = [100u32, 200u32, 300u32];
            for (i, &page_no) in test_pages.iter().enumerate() {
                fsp_seg_inode.fseg_set_nth_frag_page_no(i, page_no, &mut mtr);
            }
            let frag_pages = fsp_seg_inode.fseg_get_all_frag_pages();
            assert_eq!(frag_pages.len(), FSEG_FRAG_ARR_N_SLOTS);

            for (i, &page_no) in test_pages.iter().enumerate() {
                assert_eq!(frag_pages[i], page_no);
            }
        }
    }

    #[test]
    fn test_fseg_get_n_frag_pages() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        let mut fsp_seg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_get_nth_inode(0, &mut mtr);

        fsp_seg_inode.init(&mut mtr);

        let n_frag_pages = fsp_seg_inode.fseg_get_n_frag_pages();
        assert_eq!(n_frag_pages, 0);

        let test_pages = [1u32, 2u32, 3u32, 9u32, 19u32];
        for (index, page_no) in test_pages.iter().enumerate() {
            fsp_seg_inode.fseg_set_nth_frag_page_no(index, *page_no, &mut mtr);
        }

        let n_frag_pages = fsp_seg_inode.fseg_get_n_frag_pages();
        assert_eq!(n_frag_pages, test_pages.len());
    }

    #[test]
    fn test_fseg_find_last_used_frag_page_slot() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        let mut fsp_seg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_get_nth_inode(0, &mut mtr);

        fsp_seg_inode.init(&mut mtr);

        let n_frag_pages = fsp_seg_inode.fseg_get_n_frag_pages();
        assert_eq!(n_frag_pages, 0);

        let test_cases = [0u32, 1u32, 2u32, 3u32, 9u32, 19u32];
        for i in test_cases.iter() {
            fsp_seg_inode.fseg_set_nth_frag_page_no(*i as usize, *i, &mut mtr);
        }

        let res = fsp_seg_inode.fseg_find_last_used_frag_page_slot();
        assert_eq!(res.unwrap(), *test_cases.last().unwrap() as usize);
    }

    #[test]
    fn test_fseg_find_free_frag_page_slot() {
        let mut mtr = Mtr::new();
        mtr.start();

        let owning_page = create_fseg_inode_page();
        let page = owning_page.page;
        let fsp_seg_inode_page = FSegInodePage::new(owning_page.page);

        mtr.memo_push(MemoSlot::PageFix(
            page.page_get_page_id(),
            FixMode::Exclusive,
        ));

        let mut fsp_seg_inode =
            fsp_seg_inode_page.fsp_seg_inode_page_get_nth_inode(0, &mut mtr);

        fsp_seg_inode.init(&mut mtr);

        let n_frag_pages = fsp_seg_inode.fseg_get_n_frag_pages();
        assert_eq!(n_frag_pages, 0);

        let test_cases = [0u32, 1u32, 2u32, 3u32, 9u32, 19u32];
        for i in test_cases.iter() {
            fsp_seg_inode.fseg_set_nth_frag_page_no(*i as usize, *i, &mut mtr);
        }

        let res = fsp_seg_inode.fseg_find_free_frag_page_slot();
        assert_eq!(res.unwrap(), 4);
    }
}
