#[cfg(test)]
pub(super) mod tests {
    use std::sync::{Arc, Mutex, RwLock};
    use rstest::{fixture, rstest};

    use crate::buf::buf0buf::BufPoolInner;
    use crate::buf::buf0types::{BufBlock, BufPage, BufPageState, Frame};
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
    use crate::tests::test0utils;

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
            let inner_pool = Arc::new(Mutex::new(BufPoolInner::new()));
            let buf_pool = Arc::new(Mutex::new(BufPool::new(
                fil,
                buf_lru.clone(),
                buf_rea,
            )));

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
        let buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

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
            let resp = buf_pool.create(0, i, &mut mtr);
            assert!(resp.is_some());
        }

        for i in 1..10 {
            let page = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::SLatch,
                page_id: PageId { space_id: 0, page_no: i },
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

    #[rstest]
    fn test_block_remove_hashed_page_scrubs_header_and_updates_clocks(buf_pool_fixture: BufPoolFixture) {
        // 1) Build a fresh pool fixture.
        let pool_arc = buf_pool_fixture.buf_pool.clone();

        // 2) Pick a page id.
        let space_id = 0;
        let page_no  = 42;
        let owning_page = test0utils::create_raw_page();
        let buf_page = owning_page.page.clone();
        let block = owning_page.buf_block.clone();
        
        // 4) Initialize the page in the pool (this sets in_file & hashes it).
        {
            let mut pool = pool_arc.lock().unwrap();
            pool.page_init(owning_page.page.page_get_space_id(), page_no, block.clone());
        }

        // 5) Add it to LRU as the “young” end.
        {
            let mut pool = pool_arc.lock().unwrap();
            let mut guard = block.write().unwrap();
            pool.add_block(guard.buf_block_get_page_mut(), /*old=*/ false);
        }

        // 6) Snapshot clocks & hash‐membership before eviction.
        let (before_pool_clk, before_block_clk, in_hash_before) = {
            let pool       = pool_arc.lock().unwrap();
            let pool_clk   = pool.freed_page_clock();
            let block_clk  = block.read().unwrap().buf_block_get_clock();
            let in_hash    = pool.peek(space_id, page_no);
            (pool_clk, block_clk, in_hash)
        };
        assert!(in_hash_before, "page must start in the hash map");

        // 7) Evict via the public API.
        let returned_state = {
            let pool = pool_arc.lock().unwrap();
            pool.block_remove_hashed_page(space_id, page_no)
        };

        // 8) It should return the “removed‐hash” state.
        assert_eq!(
            returned_state,
            BufPageState::BufBlockRemoveHash,
            "should return the RemoveHash state"
        );

        // 9) Snapshot clocks & membership again.
        let (after_pool_clk, after_block_clk, in_hash_after) = {
            let pool       = pool_arc.lock().unwrap();
            let pool_clk   = pool.freed_page_clock();
            let block_clk  = block.read().unwrap().buf_block_get_clock();
            let in_hash    = pool.peek(space_id, page_no);
            (pool_clk, block_clk, in_hash)
        };

        // 10) Verify eviction, clocks, and hash removal.
        assert!(!in_hash_after, "page should no longer be in the hash");
        assert_eq!(after_pool_clk,  before_pool_clk + 1, "global clock must +1");
        assert_eq!(after_block_clk, before_block_clk + 1, "block clock must +1");

        // 11) Finally, verify that the two header fields were overwritten with 0xFF:
        {
            let page = {
                // assuming your BufBlock exposes the frame as &_[u8]_
                let guard = block.read().unwrap();
                guard.get_page()
            };
            
            // Check that exactly those 4 bytes at each offset are 0xFF
            assert_eq!(
                page.page_get_space_id(),
                0xFF,
                "space_id field must be scrubbed"
            );
            assert_eq!(
                page.page_get_fil_offset(),
                0xFF,
                "page_no field must be scrubbed"
            );
        }
    }

 
}
