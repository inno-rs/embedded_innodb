#[cfg(test)]
mod tests {
    use std::alloc::{Layout, alloc_zeroed};
    use std::cell::RefCell;
    use std::rc::Rc;

    use rstest::{fixture, rstest};
    use tracing::info;

    use super::super::page0page::Page;
    use crate::buf::buf0types::{BufBlock, BufPage, Frame};
    use crate::dict::dict0types::Index;
    use crate::innodb::innodb0types::{PageFrame, PageId, UNIV_PAGE_SIZE};
    use crate::mtr::mtr0mtr::Mtr;
    use crate::page::page0page::*;
    use crate::rem::rem0rec::{Record, RecordFieldData};
    use crate::tests::test0utils::{FieldType, create_records};

    struct PageFixture {
        index: Index,
        frame: Frame,
        buf_block: BufBlock,
        mtr: Option<Mtr>,
        page: Page,
    }

    impl PageFixture {
        fn new() -> PageFixture {
            let index = Index::new(PageId { space_id: 0, page_no: 0 });
            let frame = get_raw_block();
            let buf_page =
                RefCell::new(Rc::new(RefCell::new(BufPage::dummy())));
            let buf_block = BufBlock::new(buf_page, frame);
            let mtr = None;
            let page = Page::page_create(&index, &buf_block, mtr);

            PageFixture { index, frame, buf_block, mtr: None, page }
        }
    }

    impl Drop for PageFixture {
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
    fn page_fixture() -> PageFixture {
        PageFixture::new()
    }

    #[rstest]
    fn test_page_create(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        let infimum_rec = page.page_get_infimum_rec();
        let extract_data = |rec: Record| -> String {
            return match rec.rec_get_data(0).unwrap() {
                RecordFieldData::Data(d) => d,
                RecordFieldData::SqlNullData() => {
                    panic!("SQL NULL DATA");
                }
            };
        };

        assert_eq!(extract_data(infimum_rec), "infimumr");

        let supremum_rec = page.page_get_supremum_rec();
        info!("{:?}", supremum_rec.rec_get_n_fields());
        assert_eq!(extract_data(supremum_rec), "supremumr");

        assert_eq!(page.page_header_get_field(PAGE_N_DIR_SLOTS), 2);
        assert_eq!(
            page.page_header_get_field(PAGE_HEAP_TOP),
            PAGE_SUPREMUM_END
        );
        assert_eq!(
            page.page_header_get_field(PAGE_N_HEAP),
            PAGE_HEAP_NO_USER_LOW
        );
        assert_eq!(page.page_header_get_field(PAGE_FREE), 0);
        assert_eq!(page.page_header_get_field(PAGE_GARBAGE), 0);
        assert_eq!(
            page.page_header_get_field(PAGE_LAST_INSERT),
            u16::MAX as usize
        );
        assert_eq!(
            page.page_header_get_field(PAGE_DIRECTION),
            PAGE_NO_DIRECTION
        );
        assert_eq!(page.page_header_get_field(PAGE_N_DIRECTION), 0);
        assert_eq!(page.page_header_get_field(PAGE_N_RECS), 0);
        assert_eq!(page.get_max_trx_id(), 0);

        // Check the slots in the page header
        assert_eq!(
            page.page_dir_slot_get_rec(0).get_ptr() as usize,
            infimum_rec.get_ptr() as usize
        );

        assert_eq!(
            page.page_dir_slot_get_rec(1).get_ptr() as usize,
            supremum_rec.get_ptr() as usize
        );

        // Check next offsets of the infimum and supremum records
        assert_eq!(infimum_rec.rec_get_next_offs(), PAGE_SUPREMUM);
        assert_eq!(page.page_get_supremum_rec().rec_get_next_offs(), 0);

        assert!(page.page_header_get_page_free().is_none());
    }

    #[rstest]
    fn test_page_get_n_recs(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        assert_eq!(page.page_n_get_recs(), 0);
    }

    #[rstest]
    fn test_page_get_next_record(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();
        let supremum_next = supremum_rec.rec_get_next();

        assert!(page.page_rec_is_infimum(infimum_rec));
        assert!(page.page_rec_is_supremum(supremum_rec));

        assert_eq!(supremum_next, None);
    }

    #[rstest]
    fn test_page_rec_is_user_record(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();

        assert!(!page.page_rec_is_user_rec(infimum_rec));
        assert!(!page.page_rec_is_user_rec(supremum_rec));
    }

    #[rstest]
    fn test_page_dir_find_owner_slot(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();

        assert_eq!(page.page_dir_find_owner_slot(infimum_rec), 0);
        assert_eq!(page.page_dir_find_owner_slot(supremum_rec), 1);
    }

    #[rstest]
    fn test_page_set_max_trx_id(page_fixture: PageFixture) {
        let page = &page_fixture.page;
        let max_trx_id: crate::utils::TxnId = 2352325;

        page.set_max_trx_id(max_trx_id, None);

        let retrieved_max_trx_id = page.get_max_trx_id();
        assert_eq!(retrieved_max_trx_id, max_trx_id);
    }

    #[rstest]
    fn test_page_set_header_field(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        {
            let field_offset: usize = PAGE_N_DIR_SLOTS;
            let value = 42;

            page.page_header_set_field(field_offset, value as usize);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_HEAP_TOP;
            let value = 44;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_HEAP;
            let value = 46;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_FREE;
            let value = 47;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_GARBAGE;
            let value = 48;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LAST_INSERT;
            let value = 54;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_DIRECTION;
            let value = 56;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_DIRECTION;
            let value = 58;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_RECS;
            let value = 60;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_MAX_TRX_ID;
            let value = 62;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LEVEL;
            let value = 64;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_INDEX_ID;
            let value = 66;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }
    }

    #[test]
    fn test_page_dir_calc_reserved_space() {
        let val = Page::page_dir_calc_reserved_space(1);
        assert_eq!(val, 1);

        let val = Page::page_dir_calc_reserved_space(100);
        assert_eq!(val, 50);
    }

    #[test]
    fn test_page_get_free_space_of_empty() {
        assert_eq!(16191, Page::page_get_free_space_of_empty());
    }

    #[test]
    fn test_page_data_calc_checksum() {
        let mut my_byte_array: [u8; UNIV_PAGE_SIZE] = [0; UNIV_PAGE_SIZE];
        let page_frame = PageFrame(&mut my_byte_array);
        assert_eq!(2194025712, Page::page_data_calc_checksum(&page_frame));
    }

    #[rstest]
    fn test_page_get_max_insert_size(page_fixture: PageFixture) {
        // Empty page
        {
            let page = &page_fixture.page;

            let size = page.page_get_max_insert_size(1);
            assert_eq!(size, 16190);
        }
    }

    #[test]
    fn test_page_get_middle_rec_on_empty_page() {
        let owning_page = create_records(0, vec![]);
        let rec = owning_page.page.page_get_middle_rec();
        assert!(rec.is_none());
    }

    #[test]
    fn test_page_get_middle_rec_on_page_with_2_rec() {
        let owning_page = create_records(
            2,
            vec![FieldType::String(Some("foo".to_string()))],
        );
        let rec = owning_page.page.page_get_middle_rec();
        assert!(rec.is_some());
    }

    #[rstest]
    fn test_page_dir_add_slot(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        assert_eq!(2, page.page_dir_get_n_slots());

        page.page_dir_add_slot(1);
        assert_eq!(3, page.page_dir_get_n_slots());

        page.page_dir_add_slot(0);
        assert_eq!(4, page.page_dir_get_n_slots());

        page.page_dir_add_slot(3);
        assert_eq!(5, page.page_dir_get_n_slots());
    }
}
