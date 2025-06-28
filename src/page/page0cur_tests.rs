#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc_zeroed},
        ops::Deref,
        sync::{Arc, RwLock},
    };

    use rstest::{fixture, rstest};
    use tracing::info;

    use crate::{
        buf::buf0types::{BufBlock, BufPage, Frame},
        dict::dict0types::Index,
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        mtr::mtr0mtr::Mtr,
        page::{page0cur::PageCur, page0page::Page},
        rem::rem0rec::{OrphanRecord, Record},
        tests::test0utils::{
            FieldType, FieldType::Int, create_dtuples,
            create_dtuples_from_input_data_chunk, create_records,
        },
    };

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

    struct PageCurFixture {
        ptr: *mut u8,
        page_cur: PageCur,
        page: Page,
    }

    impl PageCurFixture {
        fn new() -> PageCurFixture {
            let frame = get_raw_block();
            let buf_page = BufPage::dummy();
            let buf_block =
                Arc::new(RwLock::new(BufBlock::new(buf_page, frame)));
            let index = Index::new(PageId::dummy());
            let page = Page::page_create(
                &index,
                buf_block.write().unwrap().deref(),
                None,
            );
            let infimum_rec = page.page_get_infimum_rec();
            PageCurFixture {
                ptr: frame,
                page_cur: PageCur::new(
                    Arc::new(Index::new(PageId::dummy())),
                    Some(infimum_rec),
                    buf_block,
                    page,
                ),
                page,
            }
        }
    }

    impl Drop for PageCurFixture {
        fn drop(&mut self) {
            free_raw_block(self.ptr);
        }
    }

    #[fixture]
    fn page_cur_fixture() -> PageCurFixture {
        PageCurFixture::new()
    }

    #[rstest]
    #[ignore]
    fn test_page_cur_get_rec(page_cur_fixture: PageCurFixture) {
        assert_eq!(
            page_cur_fixture.page_cur.page_cur_get_rec(),
            Record::new(page_cur_fixture.ptr)
        )
    }

    #[rstest]
    fn test_page_cur_insert_low(page_cur_fixture: PageCurFixture) {
        let mut mtr = Mtr::new();
        let dtuples = create_dtuples(
            1,
            vec![FieldType::String(Some("Hello World 89".to_string()))],
        );
        let dtuple = dtuples.first().unwrap();
        let n_fields = dtuple.n_fields;

        let mut orphan_record =
            OrphanRecord::new(vec![0; UNIV_PAGE_SIZE], n_fields);

        let rec: Record = orphan_record.get_record();

        let index = Index::new(PageId::dummy());
        rec.rec_convert_dtuple_to_rec(&index, &dtuple, 0);

        assert_eq!(page_cur_fixture.page.page_n_get_recs(), 0);
        let new_rec =
            page_cur_fixture.page_cur.page_cur_insert_rec_low(rec, &mut mtr);

        assert_eq!(page_cur_fixture.page.page_n_get_recs(), 1);
        assert!(new_rec.is_some());

        let rec = new_rec.unwrap();

        assert_eq!(rec.rec_get_n_fields(), 1);
    }

    #[rstest]
    fn test_page_cur_insert_multiple(mut page_cur_fixture: PageCurFixture) {
        let mut mtr = Mtr::new();
        let index = Index::new(PageId::dummy());

        let owing_page = create_records(
            10,
            vec![FieldType::String(Some("Hello World 9".to_string()))],
        );

        assert_eq!(page_cur_fixture.page.page_n_get_recs(), 0);

        let mut n_recs = 0;
        for rec in owing_page.page.page_get_user_records() {
            n_recs += 1;
            let new_rec = page_cur_fixture
                .page_cur
                .page_cur_insert_rec_low(rec, &mut mtr);
            assert!(new_rec.is_some());
            assert_eq!(page_cur_fixture.page.page_n_get_recs(), n_recs);

            page_cur_fixture.page_cur.page_cur_move_to_next();

            let new_rec = new_rec.unwrap();
            assert_eq!(new_rec.rec_get_n_fields(), 1);
            info!("rec={}", new_rec);
        }
    }

    #[test]
    fn test_page_cur_delete_rec_single() {
        let mut mtr = Mtr::new();
        let index = Arc::new(Index::new(PageId::dummy()));

        let owning_page = create_records(
            1,
            vec![FieldType::String(Some("Hello World 9".to_string()))],
        );
        let page = owning_page.page;

        assert_eq!(page.page_get_n_recs(), 1);
        assert_eq!(page.page_get_user_records().len(), 1);

        let mut page_cur = PageCur::new(
            index,
            Some(page.page_get_user_records()[0]),
            owning_page.buf_block.clone(),
            page,
        );

        page_cur.page_cur_delete_rec(Some(&mut mtr));

        assert_eq!(page.page_get_n_recs(), 0);
        assert_eq!(page.page_get_user_records().len(), 0);
    }

    #[test]
    fn test_page_cur_delete_rec_multiple() {
        let mut mtr = Mtr::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let owning_page = create_records(0, vec![]);
        let page = owning_page.page;
        let mut page_cur = PageCur::new(
            index.clone(),
            None,
            owning_page.buf_block.clone(),
            page,
        );

        let page = owning_page.page;
        let limit = 700;

        for i in 0..limit {
            let dtuples = create_dtuples_from_input_data_chunk(vec![[
                FieldType::String(Some(format!("hello world {i}"))),
            ]]);

            page_cur.page_cur_tuple_insert(
                &dtuples[0],
                index.deref(),
                0,
                &mut mtr,
            );
        }

        assert_eq!(page.page_get_n_recs(), limit);
        assert_eq!(page.page_get_user_records().len(), limit);

        for rec in owning_page.page.page_get_user_records() {
            page_cur.page_cur_position(rec);
            page_cur.page_cur_delete_rec(Some(&mut mtr));
        }

        assert_eq!(page.page_get_n_recs(), 0);
        assert_eq!(page.page_get_user_records().len(), 0);
    }

    #[test]
    fn test_page_cur_delete_moves_to_next_rec() {
        let limit = 47;
        let mut mtr = Mtr::new();
        let index = Arc::new(Index::new(PageId::dummy()));
        let owning_page = create_records(0, vec![]);
        let page = owning_page.page;
        let mut page_cur = PageCur::new(
            index.clone(),
            None,
            owning_page.buf_block.clone(),
            page,
        );

        for i in 0..limit {
            let dtuples = create_dtuples_from_input_data_chunk(vec![[
                FieldType::String(Some(format!(
                    "hello world {:05}",
                    limit - i
                ))),
            ]]);

            page_cur.page_cur_set_before_first();
            page_cur.page_cur_tuple_insert(
                &dtuples[0],
                index.deref(),
                0,
                &mut mtr,
            );
        }

        page_cur.page_cur_set_before_first();
        page_cur.page_cur_move_to_next();

        for i in 0..limit {
            page_cur.page_cur_delete_rec(Some(&mut mtr));

            let row_data = page_cur
                .page_cur_get_rec()
                .rec_get_data(0)
                .unwrap()
                .data_or_panic();
            if i < limit - 1 {
                assert_eq!(row_data, format!("hello world {:05}", i + 2))
            } else {
                // last data should be supremum
                assert_eq!(row_data, "supremumr")
            }
        }

        assert_eq!(page.page_get_n_recs(), 0);
    }

    #[test]
    fn test_page_cur_is_before_first() {
        let index = Arc::new(Index::new(PageId::dummy()));
        let owning_page = create_records(20, vec![Int(Some(45))]);
        let mut page_cur = PageCur::new(
            index,
            None,
            owning_page.buf_block.clone(),
            owning_page.page,
        );

        page_cur.page_cur_set_before_first();

        assert!(
            owning_page.page.page_rec_is_infimum(page_cur.page_cur_get_rec())
        );
    }

    #[test]
    fn test_page_cur_is_after_last() {
        let index = Arc::new(Index::new(PageId::dummy()));
        let owning_page = create_records(20, vec![Int(Some(45))]);
        let mut page_cur = PageCur::new(
            index,
            None,
            owning_page.buf_block.clone(),
            owning_page.page,
        );

        page_cur.page_cur_set_after_last();

        assert!(
            owning_page.page.page_rec_is_supremum(page_cur.page_cur_get_rec())
        );
    }
}
