#[cfg(test)]
mod tests {
    use crate::buf::buf0types::{BufBlock, Frame};
    use crate::dict::dict0types::Index;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::mtr::mtr0mtr::Mtr;
    use crate::page::page0cur::PageCur;
    use crate::page::page0page::Page;
    use crate::rem::rem0rec::{OrphanRecord, Record};
    use crate::tests::test0utils::{create_dtuples, FieldType};
    use rstest::{fixture, rstest};
    use std::alloc::{alloc_zeroed, Layout};
    use std::sync::Arc;

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
            let buf_block = BufBlock::new(frame);
            let index = Index::new();
            let page = Page::page_create(&index, &buf_block, None);
            let infimum_rec = page.page_get_infimum_rec();
            PageCurFixture {
                ptr: frame,
                page_cur: PageCur::new(
                    Arc::new(Index::new()),
                    infimum_rec,
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
        {
            let mut mtr = Mtr::new();
            let dtuples = create_dtuples(1, vec![FieldType::String]);
            let owning_dtuple = dtuples.first().unwrap();
            let n_fields = owning_dtuple.dtuple.n_fields;

            let mut orphan_record =
                OrphanRecord::new(vec![0; UNIV_PAGE_SIZE], n_fields);

            let rec: Record = orphan_record.get_record();

            let index = Index::new();
            rec.rec_convert_dtuple_to_rec(&index, &owning_dtuple.dtuple, 0);

            assert_eq!(page_cur_fixture.page.page_n_get_recs(), 0);
            let new_rec = page_cur_fixture
                .page_cur
                .page_cur_insert_rec_low(rec, &mut mtr);

            assert_eq!(page_cur_fixture.page.page_n_get_recs(), 1);
            assert!(new_rec.is_some());

            let rec = new_rec.unwrap();

            assert_eq!(rec.rec_get_n_fields(), 1);
        }
    }
}
