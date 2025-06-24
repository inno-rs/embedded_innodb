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
        tests::test0utils::{FieldType, create_dtuples, create_records},
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
        for rec in owing_page.page.get_user_records() {
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
}
