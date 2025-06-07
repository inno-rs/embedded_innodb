#[cfg(test)]
pub(super) mod tests {
    use crate::buf::buf0buf::BufPool;
    use crate::buf::buf0lru::BufLRU;
    use crate::fil::fil0fil::Fil;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::mtr::mtr0mtr::Mtr;
    use crate::mtr::mtr0mtr::MtrState::MTR_ACTIVE;
    use crate::os::os0io::OsIo;
    use rstest::{fixture, rstest};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, RwLock};

    pub struct BufPoolFixture {
        buf_lru: Arc<RefCell<BufLRU>>,
        buf_pool: Rc<RefCell<BufPool>>,
    }

    impl BufPoolFixture {
        pub fn new() -> Self {
            let os_io = Arc::new(Mutex::new(OsIo::new()));
            let fil = Arc::new(RwLock::new(Fil::new(1, os_io)));
            let buf_lru = Arc::new(RefCell::new(BufLRU::new()));
            let buf_pool_rc =
                Rc::new(RefCell::new(BufPool::new(fil, buf_lru.clone())));
            buf_lru.borrow_mut().init(Rc::downgrade(&buf_pool_rc));

            BufPoolFixture { buf_lru, buf_pool: buf_pool_rc }
        }
    }

    #[fixture]
    fn buf_pool_fixture() -> BufPoolFixture {
        BufPoolFixture::new()
    }

    #[rstest]
    fn create_buf_pool(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.borrow_mut();

        let page = buf_pool.hash_get_page(0, 0);

        assert!(page.is_none());
        assert!(!buf_pool.peek(0, 1));
    }

    #[rstest]
    fn test_free_list_returns_none(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.borrow_mut();

        let r = buf_pool.get_free_block();
        assert!(r.is_none());
    }

    #[rstest]
    fn test_free_list_returns_some(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.borrow_mut();

        let result = buf_pool.chunk_init(10 * UNIV_PAGE_SIZE);

        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(result.read().unwrap().size, 10);
    }

    #[rstest]
    fn test_create_space(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.borrow_mut();
        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MTR_ACTIVE);

        // Try creating when there are no pages in the pool.
        let result = buf_pool.create(0, 0, &mut mtr);

        assert!(result.is_none());

        // Add some pages, and then retry creating.
        let _result = buf_pool.chunk_init(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);

        let resp = buf_pool.create(0, 0, &mut mtr);
        assert!(resp.is_some());

        // The total number of items in the free list should decrease by 1.
        assert_eq!(buf_pool.get_free_list_size(), 9);

        for i in 0..9 {
            let resp = buf_pool.create(0, 0, &mut mtr);
            assert!(resp.is_some());

            assert_eq!(buf_pool.get_free_list_size(), 9 - i - 1);
        }
    }
}
