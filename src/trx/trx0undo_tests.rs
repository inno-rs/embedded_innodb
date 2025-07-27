#[cfg(test)]
mod tests {
    use std::alloc::{Layout, alloc_zeroed};

    use pretty_assertions::assert_eq;
    use rstest::{fixture, rstest};

    use crate::{
        buf::buf0types::{BufBlock, BufPage, Frame},
        dict::dict0types::Index,
        fil::fil0types::FSEG_PAGE_DATA,
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        mach::mach0data::mach_write_to_2,
        page::page0page::Page,
        trx::trx0undo::{
            TRX_UNDO_LOG_START, TRX_UNDO_PAGE_HDR_SIZE, TrxUndo, UndoPage,
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
}
