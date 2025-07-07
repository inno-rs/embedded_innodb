#[cfg(test)]
pub(super) mod tests {
    use std::sync::{Arc, Mutex};

    use rstest::{fixture, rstest};

    use crate::{
        buf::{
            buf0buf::BufPool,
            buf0lru::BufLRU,
            buf0rea::BufRea,
            buf0types::{BUF_GET_IF_IN_POOL, Request},
        },
        fil::fil0fil::Fil,
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        mtr::mtr0mtr::{Mtr, MtrState::MtrActive},
        os::{os0io::OsIo, os0runtime::create_runtime},
        sync::sync0rw::ReadWriteLatch,
    };

    pub struct BufPoolFixture {
        buf_pool: Arc<Mutex<BufPool>>,
    }

    impl BufPoolFixture {
        pub fn new() -> Self {
            let runtime = create_runtime(1);
            let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
            let fil = Arc::new(Mutex::new(Fil::new(1, os_io, runtime)));
            let buf_rea = Arc::new(Mutex::new(BufRea::new(fil.clone())));
            let buf_lru = Arc::new(Mutex::new(BufLRU::new()));
            let buf_pool = Arc::new(Mutex::new(BufPool::new(
                fil,
                buf_lru.clone(),
                buf_rea,
            )));
            buf_lru.lock().unwrap().init(Arc::downgrade(&buf_pool));

            BufPoolFixture { buf_pool }
        }
    }

    #[fixture]
    fn buf_pool_fixture() -> BufPoolFixture {
        BufPoolFixture::new()
    }

    #[rstest]
    fn create_buf_pool(buf_pool_fixture: BufPoolFixture) {
        let buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let page = buf_pool.hash_get_page(0, 0);

        assert!(page.is_none());
        assert!(!buf_pool.peek(0, 1));
    }

    #[rstest]
    fn test_free_list_returns_none(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let r = buf_pool.get_free_block();
        assert!(r.is_none());
    }

    #[rstest]
    fn test_free_list_returns_some(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        buf_pool.open(10 * UNIV_PAGE_SIZE);

        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);
    }

    #[rstest]
    fn test_create_space(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        // Try creating when there are no pages in the pool.
        let result = buf_pool.create(0, 0, &mut mtr);

        assert!(result.is_none());

        // Add some pages, and then retry creating.
        buf_pool.open(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);

        let resp = buf_pool.create(0, 0, &mut mtr);
        assert!(resp.is_some());

        // The total number of items in the free list should decrease by 1.
        assert_eq!(buf_pool.get_free_list_size(), 9);

        for i in 1..10 {
            let resp = buf_pool.create(i, i, &mut mtr);
            assert!(resp.is_some());

            assert_eq!(buf_pool.get_free_list_size(), 9 - (i as usize));
        }
    }

    #[rstest]
    fn test_buf_pool_get_page(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        // Initialize a chunk to have some pages.
        buf_pool.open(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);

        let resp = buf_pool.create(0, 0, &mut mtr);
        assert!(resp.is_some());

        // Get a page from the pool.
        let page = buf_pool.get(Request {
            rw_latch: ReadWriteLatch::SLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_IF_IN_POOL,
            mtr: &mut mtr,
        });
        assert!(page.is_some());
    }

    #[rstest]
    fn test_buf_pool_get_multiple(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        // Add some pages, and then retry creating.
        buf_pool.open(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);

        for i in 0..10 {
            let resp = buf_pool.create(i, i, &mut mtr);
            assert!(resp.is_some());
        }

        for i in 1..10 {
            let page = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::SLatch,
                page_id: PageId { space_id: i, page_no: i },
                mode: BUF_GET_IF_IN_POOL,
                mtr: &mut mtr,
            });
            assert!(
                page.is_some(),
                "Failed to get page for space_id: {}, page_no: {}",
                i,
                i
            );
        }
    }
}
