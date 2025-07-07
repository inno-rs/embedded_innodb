#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc_zeroed},
        collections::HashMap,
        ops::Add,
        sync::Arc,
    };

    use rand::{
        SeedableRng,
        prelude::{SliceRandom, StdRng},
    };
    use rstest::{fixture, rstest};
    use tracing::info;

    use super::super::page0page::Page;
    use crate::{
        buf::buf0types::{BufBlock, BufPage, Frame},
        dict::dict0types::Index,
        innodb::innodb0types::{PageFrame, PageId, UNIV_PAGE_SIZE},
        mtr::mtr0mtr::Mtr,
        page::page0page::*,
        rem::rem0rec::{Record, RecordFieldData},
        tests::test0utils::{FieldType, create_records},
    };

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
            let buf_page = BufPage::dummy();
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
        assert_eq!(page.page_dir_get_n_heap(), PAGE_HEAP_NO_USER_LOW);
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
        assert_eq!(page.page_get_max_trx_id(), 0);

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

        // Check heap no of the infimum record and supremum record
        assert_eq!(infimum_rec.rec_get_heap_no(), 0);
        assert_eq!(supremum_rec.rec_get_heap_no(), 1);

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
    fn test_page_dir_get_nth_slot(page_fixture: PageFixture) {
        let page = &page_fixture.page;
        let mut expected = UNIV_PAGE_SIZE - PAGE_DIR - PAGE_DIR_SLOT_SIZE;
        for i in 0..100 {
            let slot_offset = page.page_dir_get_nth_slot(i);
            assert_eq!(expected, slot_offset);
            expected -= PAGE_DIR_SLOT_SIZE;
        }
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
    fn test_page_set_max_trx_id(page_fixture: PageFixture) {
        let page = &page_fixture.page;
        let max_trx_id: crate::utils::TxnId = 2352325;

        page.page_set_max_trx_id(max_trx_id, None);

        let retrieved_max_trx_id = page.page_get_max_trx_id();
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
            let value = 46;

            page.page_dir_set_n_heap(value);

            let retrieved_value = page.page_dir_get_n_heap();
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

    #[test]
    fn test_page_move_rec_list_end() {
        let n_recs = 30;
        for i in 0..n_recs {
            let mut mtr = Mtr::new();
            let index = Arc::new(Index::new(PageId::dummy()));
            let src_owning_page = create_records(
                n_recs,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            let dest_owning_page = create_records(0, vec![]);

            let src_page = src_owning_page.page;
            let dest_page = dest_owning_page.page;
            let rec = src_page.page_get_user_records()[i];

            assert_eq!(src_page.page_get_user_records().len(), n_recs);

            src_page.page_move_rec_list_end(
                dest_owning_page.buf_block.clone(),
                rec,
                index,
                &mut mtr,
            );

            assert_eq!(src_page.page_get_user_records().len(), i);
            assert_eq!(dest_page.page_get_user_records().len(), n_recs - i);
        }
    }

    #[test]
    fn test_page_delete_rec_list_start() {
        let n_recs = 30;
        for i in 0..n_recs {
            let mut mtr = Mtr::new();
            let index = Arc::new(Index::new(PageId::dummy()));
            let owning_page = create_records(
                n_recs,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            let mut page = owning_page.page;
            let rec = page.page_get_user_records()[i];

            assert_eq!(page.page_get_user_records().len(), n_recs);

            // deletion does not delete the record being pointed to by the page cursor.
            page.page_delete_rec_list_start(
                rec,
                owning_page.buf_block.clone(),
                index,
                &mut mtr,
            );

            // Since the record being pointed to is not being deleted, the number of records after the first deletion does not change the number of records.
            assert_eq!(page.page_get_user_records().len(), n_recs - i);
        }
    }

    #[test]
    fn test_page_delete_rec_list_start_when_rec_points_to_supremum() {
        let n_recs = 45;
        let mut mtr = Mtr::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let src_owning_page = create_records(
            n_recs,
            vec![FieldType::String(Some("foo".to_string()))],
        );

        let mut page = src_owning_page.page;

        assert_eq!(page.page_get_user_records().len(), n_recs);

        page.page_delete_rec_list_start(
            page.page_get_supremum_rec(),
            src_owning_page.buf_block.clone(),
            index,
            &mut mtr,
        );

        assert_eq!(page.page_get_user_records().len(), 0);
    }

    #[test]
    fn test_page_delete_rec_list_start_when_rec_points_to_infimum() {
        let n_recs = 45;
        let mut mtr = Mtr::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let src_owning_page = create_records(
            n_recs,
            vec![FieldType::String(Some("foo".to_string()))],
        );

        let mut page = src_owning_page.page;

        assert_eq!(page.page_get_user_records().len(), n_recs);

        page.page_delete_rec_list_start(
            page.page_get_infimum_rec(),
            src_owning_page.buf_block.clone(),
            index,
            &mut mtr,
        );

        assert_eq!(page.page_get_user_records().len(), n_recs);
    }

    #[test]
    fn test_page_n_owned() {
        let owning_page = create_records(
            100,
            vec![FieldType::String(Some("foo".to_string()))],
        );

        let page = owning_page.page;

        let mut slot_recs = vec![];

        let n_slots = page.page_dir_get_n_slots();
        // First slot == infimum
        // Last slot == supremum
        for slot_id in 1..n_slots - 1 {
            let rec = page.page_dir_slot_get_rec(slot_id);
            slot_recs.push(rec);
        }

        let mut n_owned_recs = vec![];
        for rec in page.page_get_user_records() {
            if rec.rec_get_n_owned() != 0 {
                n_owned_recs.push(rec);
            }
        }

        let rec_cmp = |x: &Record, y: &Record| x.get_ptr().cmp(&y.get_ptr());

        slot_recs.sort_by(rec_cmp);
        n_owned_recs.sort_by(rec_cmp);

        assert_eq!(slot_recs, n_owned_recs);
    }

    /* START PAGE DIRECTORY TESTS */

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

    #[rstest]
    fn test_page_dir_find_owner_slot_on_empty_page(page_fixture: PageFixture) {
        let page = &page_fixture.page;

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();

        assert_eq!(page.page_dir_find_owner_slot(infimum_rec), 0);
        assert_eq!(page.page_dir_find_owner_slot(supremum_rec), 1);
    }

    #[test]
    fn test_page_dir_find_owner_slot_with_multiple_records() {
        // Set a deterministic seed for reproducibility
        let seed = 42;
        let mut rng = StdRng::seed_from_u64(seed);

        let mut range: Vec<usize> = (1..=200).collect();
        range.shuffle(&mut rng);
        let result: Vec<usize> = range.into_iter().take(10).collect();

        for total_records in result {
            let owning_page = create_records(
                total_records,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            let page = owning_page.page;

            let mut slot_map = HashMap::<SlotId, Vec<Record>>::new();

            for rec in page.page_get_user_records() {
                let slot_id = page.page_dir_find_owner_slot(rec);
                slot_map.entry(slot_id).or_default().push(rec);
            }

            let mut total_recs_from_slot_map = 0;
            let mut recs = Vec::<Record>::new();
            for (_key, value) in slot_map {
                total_recs_from_slot_map =
                    total_recs_from_slot_map.add(value.len());
                recs.extend(value);
            }

            assert_eq!(total_recs_from_slot_map, total_records);
            recs.sort_by(|x, y| x.get_ptr().cmp(&y.get_ptr()));

            let mut user_recs = owning_page.page.page_get_user_records();
            user_recs.sort_by(|x, y| x.get_ptr().cmp(&y.get_ptr()));

            assert_eq!(recs, user_recs);
        }
    }

    #[rstest]
    fn test_page_directory_on_empty_page(page_fixture: PageFixture) {
        let page = page_fixture.page;

        assert_eq!(2, page.page_dir_get_n_slots());

        let rec = page.page_dir_slot_get_rec(0);
        assert!(page.page_rec_is_infimum(rec));
        assert_eq!(page.page_dir_find_owner_slot(rec), 0);

        let rec = page.page_dir_slot_get_rec(1);
        assert!(page.page_rec_is_supremum(rec));
        assert_eq!(page.page_dir_find_owner_slot(rec), 1);
    }

    #[test]
    fn test_page_directory_slot_size() {
        // If the page has 7 records, the supremum will own only 8 recs. This will not lead to splitting.
        {
            let owning_page = create_records(
                7,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            assert_eq!(owning_page.page.page_dir_get_n_slots(), 2);
        }

        {
            let owning_page = create_records(
                11,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            assert_eq!(owning_page.page.page_dir_get_n_slots(), 3);
        }

        {
            let owning_page = create_records(
                20,
                vec![FieldType::String(Some("foo".to_string()))],
            );

            assert_eq!(owning_page.page.page_dir_get_n_slots(), 6);
        }
    }

    /* END PAGE DIRECTORY TESTS */
}
