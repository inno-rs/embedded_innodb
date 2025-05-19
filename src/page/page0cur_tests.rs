#[cfg(test)]
mod tests {
    use crate::buf::buf0types::{BufBlock, Frame};
    use crate::dict::dict0types::Index;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::page::page0cur::PageCur;
    use crate::rem::rem0rec::Record;
    use rstest::{fixture, rstest};
    use std::alloc::{alloc_zeroed, Layout};
    use std::sync::{Arc, Mutex};

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
    }

    impl PageCurFixture {
        fn new() -> PageCurFixture {
            let block = get_raw_block();
            PageCurFixture {
                ptr: block,
                page_cur: PageCur::new(
                    Arc::new(Mutex::new(Index::new())),
                    Record::new(block),
                    BufBlock::new(block),
                ),
            }
        }
    }

    impl Drop for PageCurFixture {
        fn drop(&mut self) {
            free_raw_block(self.ptr);
        }
    }

    #[fixture]
    fn context() -> PageCurFixture {
        PageCurFixture::new()
    }

    #[rstest]
    fn test_page_cur_get_rec(context: PageCurFixture) {
        assert_eq!(
            context.page_cur.page_cur_get_rec(),
            Record::new(context.ptr)
        )
    }
}
