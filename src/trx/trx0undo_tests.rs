#[cfg(test)]
mod tests {
    use std::alloc::{Layout, alloc_zeroed};

    use pretty_assertions::assert_eq;
    use rstest::{fixture, rstest};

    use crate::{
        buf::buf0types::{BufBlock, BufPage, Frame},
        dict::dict0types::Index,
        fil::fil0types::{FIL_PAGE_TYPE, FSEG_PAGE_DATA, FilPageType},
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        mach::mach0data::{mach_read_from_2, mach_write_to_2},
        mtr::mtr0mtr::Mtr,
        page::page0page::Page,
        trx::trx0undo::{
            TRX_UNDO_LOG_START, TRX_UNDO_PAGE_FREE, TRX_UNDO_PAGE_HDR_SIZE,
            TRX_UNDO_PAGE_START, TRX_UNDO_PAGE_TYPE, TrxUndo, TrxUndoType,
            UndoPage,
        },
    };

    // Define private constants from trx0undo.rs
    const TRX_UNDO_PAGE_HDR: usize = FSEG_PAGE_DATA;

    struct UndoPageFixture {
        undo_page: UndoPage,
        frame: Frame,
        page_no: u32,
    }

    impl UndoPageFixture {
        fn new(page_no: u32) -> Self {
            let frame = get_raw_block();
            let buf_page = BufPage::dummy();
            let buf_block = BufBlock::new(buf_page, frame);
            let index = Index::new(PageId { space_id: 0, page_no });
            let mut page = Page::page_create(&index, &buf_block, None);
            page.page_set_fil_offset(page_no as usize);
            let undo_page = UndoPage::new(page);

            UndoPageFixture { undo_page, frame, page_no }
        }
    }

    impl Drop for UndoPageFixture {
        fn drop(&mut self) {
            free_raw_block(self.frame);
        }
    }

    fn get_raw_block() -> Frame {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { alloc_zeroed(layout) }
    }

    fn free_raw_block(frame: Frame) {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { std::alloc::dealloc(frame, layout) }
    }

    #[fixture]
    fn undo_page_fixture() -> UndoPageFixture {
        UndoPageFixture::new(1)
    }

    #[fixture]
    fn trx_undo() -> TrxUndo {
        TrxUndo::new()
    }

    #[rstest]
    fn test_trx_undo_page_get_start_same_page(
        undo_page_fixture: UndoPageFixture,
        trx_undo: TrxUndo,
    ) {
        // Setup: Set a start offset in the undo page header
        let expected_start = TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE + 10; // Some arbitrary offset after the header
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        let offset = TRX_UNDO_PAGE_HDR; // Offset to the undo log header

        // Write the start offset to the undo log header
        mach_write_to_2(
            &mut page_frame.0[offset + TRX_UNDO_LOG_START..],
            expected_start as u16,
        );

        // Action: Call the function with the same page number
        let result = trx_undo.trx_undo_page_get_start(
            &undo_page_fixture.undo_page,
            undo_page_fixture.page_no,
            offset,
        );

        // Assert: The result should be the expected start offset
        assert_eq!(result, expected_start);
    }

    #[rstest]
    fn test_trx_undo_page_get_start_different_page(
        undo_page_fixture: UndoPageFixture,
        trx_undo: TrxUndo,
    ) {
        // Setup: Use a different page number than the fixture's page number
        let different_page_no = undo_page_fixture.page_no + 1;
        let offset = TRX_UNDO_PAGE_HDR; // Offset to the undo log header

        // Action: Call the function with a different page number
        let result = trx_undo.trx_undo_page_get_start(
            &undo_page_fixture.undo_page,
            different_page_no,
            offset,
        );

        // Assert: The result should be the default start position
        assert_eq!(result, TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE);
    }

    #[rstest]
    fn test_trx_undo_page_get_end_same_page_non_zero(
        undo_page_fixture: UndoPageFixture,
        trx_undo: TrxUndo,
    ) {
        // Setup: Set a non-zero end offset in the undo page
        let expected_end = 500; // Some arbitrary offset
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        let offset = 100; // Offset to check

        // Write the end offset to the page at the specified offset
        mach_write_to_2(&mut page_frame.0[offset..], expected_end as u16);

        // Action: Call the function with the same page number
        let result = trx_undo.trx_undo_page_get_end(
            &undo_page_fixture.undo_page,
            undo_page_fixture.page_no,
            offset,
        );

        // Assert: The result should be the expected end offset
        assert_eq!(result, expected_end);
    }

    #[rstest]
    fn test_trx_undo_page_get_end_same_page_zero(
        undo_page_fixture: UndoPageFixture,
        trx_undo: TrxUndo,
    ) {
        // Setup: Set a zero end offset in the undo page
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        let offset = 100; // Offset to check
        let free_space = 800; // Some arbitrary free space offset

        // Write zero to the end offset
        mach_write_to_2(&mut page_frame.0[offset..], 0);

        // Mock the free space pointer
        let free_offset = TRX_UNDO_PAGE_HDR + 4; // TRX_UNDO_PAGE_FREE offset
        mach_write_to_2(&mut page_frame.0[free_offset..], free_space as u16);

        // Action: Call the function with the same page number
        let result = trx_undo.trx_undo_page_get_end(
            &undo_page_fixture.undo_page,
            undo_page_fixture.page_no,
            offset,
        );

        // Assert: The result should be the free space pointer
        assert_eq!(result, free_space);
    }

    #[rstest]
    fn test_trx_undo_page_get_end_different_page(
        undo_page_fixture: UndoPageFixture,
        trx_undo: TrxUndo,
    ) {
        // Setup: Use a different page number than the fixture's page number
        let different_page_no = undo_page_fixture.page_no + 1;
        let offset = 100; // Offset to check
        let free_space = 800; // Some arbitrary free space offset

        // Mock the free space pointer
        let free_offset = TRX_UNDO_PAGE_HDR + 4; // TRX_UNDO_PAGE_FREE offset
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        mach_write_to_2(&mut page_frame.0[free_offset..], free_space as u16);

        // Action: Call the function with a different page number
        let result = trx_undo.trx_undo_page_get_end(
            &undo_page_fixture.undo_page,
            different_page_no,
            offset,
        );

        // Assert: The result should be the free space pointer
        assert_eq!(result, free_space);
    }

    #[rstest]
    fn test_undo_page_get_first_free(undo_page_fixture: UndoPageFixture) {
        // Setup: Set a specific free space offset in the undo page header
        let expected_free_offset = 500; // Some arbitrary offset
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        let free_offset = TRX_UNDO_PAGE_HDR + 4; // TRX_UNDO_PAGE_FREE offset

        // Write the free space offset to the header
        mach_write_to_2(
            &mut page_frame.0[free_offset..],
            expected_free_offset as u16,
        );

        // Action: Call page_get_first_free
        let result = undo_page_fixture.undo_page.page_get_first_free();

        // Assert: The result should match the expected free offset
        assert_eq!(result, expected_free_offset);
    }

    #[rstest]
    fn test_undo_page_set_first_free(mut undo_page_fixture: UndoPageFixture) {
        // Setup: Define a new free space offset to set
        let new_free_offset = 600; // Some arbitrary offset

        // Action: Call page_set_first_free
        undo_page_fixture.undo_page.page_set_first_free(new_free_offset);

        // Verify: The free space offset in the header should be updated
        let result = undo_page_fixture.undo_page.page_get_first_free();

        // Assert: The result should match the new free offset we set
        assert_eq!(result, new_free_offset);
    }

    #[rstest]
    fn test_undo_page_get_undo_type(undo_page_fixture: UndoPageFixture) {
        // Setup: Set the undo type in the page header
        let page_frame = undo_page_fixture.undo_page.page_to_frame();
        let type_offset = TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_TYPE;

        // Test for UndoInsert type
        mach_write_to_2(
            &mut page_frame.0[type_offset..],
            TrxUndoType::UndoInsert as u16,
        );

        // Action: Get the undo type
        let result = undo_page_fixture.undo_page.page_get_undo_type();

        // Assert: Should be UndoInsert
        assert_eq!(result, TrxUndoType::UndoInsert);

        // Test for UndoUpdate type
        mach_write_to_2(
            &mut page_frame.0[type_offset..],
            TrxUndoType::UndoUpdate as u16,
        );

        // Action: Get the undo type again
        let result = undo_page_fixture.undo_page.page_get_undo_type();

        // Assert: Should be UndoUpdate
        assert_eq!(result, TrxUndoType::UndoUpdate);

        // Test for unknown type
        mach_write_to_2(&mut page_frame.0[type_offset..], 999); // Some invalid value

        // Action: Get the undo type again
        let result = undo_page_fixture.undo_page.page_get_undo_type();

        // Assert: Should be Unknown
        assert_eq!(result, TrxUndoType::Unknown);
    }

    #[rstest]
    fn test_trx_undo_page_init(mut undo_page_fixture: UndoPageFixture) {
        // Setup: Create a mini-transaction for logging
        let mut mtr = Mtr::new();

        // Action: Initialize the undo page with UndoInsert type
        undo_page_fixture
            .undo_page
            .trx_undo_page_init(TrxUndoType::UndoInsert, &mut mtr);

        // Verify: Check that the page was initialized correctly
        let page_frame = undo_page_fixture.undo_page.page_to_frame();

        // 1. Check the undo page type
        let type_val = mach_read_from_2(
            &page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_TYPE..],
        );
        assert_eq!(type_val, TrxUndoType::UndoInsert as u16);

        // 2. Check the start pointer
        let start_val = mach_read_from_2(
            &page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_START..],
        );
        assert_eq!(
            start_val as usize,
            TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE
        );

        // 3. Check the free pointer
        let free_val = mach_read_from_2(
            &page_frame.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_FREE..],
        );
        assert_eq!(
            free_val as usize,
            TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_HDR_SIZE
        );

        // 4. Check the page type
        let page_type = mach_read_from_2(&page_frame.0[FIL_PAGE_TYPE..]);
        assert_eq!(page_type, FilPageType::UndoLog as u16);

        // 5. Repeat with UndoUpdate type
        let mut undo_page_fixture2 = UndoPageFixture::new(2);
        let mut mtr2 = Mtr::new();

        undo_page_fixture2
            .undo_page
            .trx_undo_page_init(TrxUndoType::UndoUpdate, &mut mtr2);

        let page_frame2 = undo_page_fixture2.undo_page.page_to_frame();
        let type_val2 = mach_read_from_2(
            &page_frame2.0[TRX_UNDO_PAGE_HDR + TRX_UNDO_PAGE_TYPE..],
        );
        assert_eq!(type_val2, TrxUndoType::UndoUpdate as u16);
    }
}
