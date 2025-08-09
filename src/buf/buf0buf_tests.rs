#[cfg(test)]
pub(super) mod tests {
    use std::sync::{Arc, Mutex, atomic::Ordering::Relaxed};

    use rstest::{fixture, rstest};

    use crate::{
        buf::{
            buf0buf::BufPool,
            buf0dblwr::{
                DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
            },
            buf0flu::BufFlush,
            buf0lru::BufLRU,
            buf0rea::BufRea,
            buf0types::{BUF_GET_IF_IN_POOL, BufIoFix, BufPageState, Request},
        },
        fil::{
            fil0fil::Fil,
            fil0types::{
                FIL_PAGE_FILE_FLUSH_LSN, FIL_PAGE_NEXT, FIL_PAGE_PREV,
                FIL_PAGE_TYPE, FilPageType,
            },
        },
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        log::log0log::Log,
        mtr::{
            mtr0mtr::Mtr,
            mtr0types::{FixMode, MemoSlot, MtrState, MtrState::MtrActive},
        },
        os::{os0io::OsIo, os0runtime::create_runtime},
        sync::sync0rw::ReadWriteLatch,
        tests::test0utils,
    };

    pub struct BufPoolFixture {
        buf_pool: Arc<Mutex<BufPool>>,
    }

    impl BufPoolFixture {
        pub fn new() -> Self {
            let runtime = create_runtime(1);
            let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
            let fil = Arc::new(Mutex::new(Fil::new(1, os_io, runtime)));
            let block = Box::new(WriteBuffer([0u8; DOUBLE_WRITE_BUFFER_SIZE]));
            let dblwr_buf = Arc::new(Mutex::new(DoubleWriteBuffer::new(
                block,
                fil.clone(),
                UNIV_PAGE_SIZE,
            )));
            let redo_log = Arc::new(Mutex::new(Log::new(
                UNIV_PAGE_SIZE,
                fil.clone(),
                Default::default(),
            )));
            let flusher = Arc::new(Mutex::new(BufFlush::new(
                dblwr_buf.clone(),
                fil.clone(),
                redo_log.clone(),
            )));
            let buf_rea = Arc::new(Mutex::new(BufRea::new(fil.clone())));
            let buf_lru = Arc::new(Mutex::new(BufLRU::new()));
            let buf_pool = Arc::new(Mutex::new(BufPool::new(
                fil,
                buf_lru.clone(),
                buf_rea,
                flusher.clone(),
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

    // TODO Implement this test. The current test is not valid anymore.
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
        mtr.start();

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
    fn test_buf_pool_get_page_with_latch(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        // Initialize a chunk to have some pages.
        buf_pool.open(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);

        // Get a page from the pool with S latch
        {
            let resp = buf_pool.create(0, 0, &mut mtr);
            assert!(resp.is_some());

            let page = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::SLatch,
                page_id: PageId { space_id: 0, page_no: 0 },
                mode: BUF_GET_IF_IN_POOL,
                mtr: &mut mtr,
            });
            assert!(page.is_some());
            assert_eq!(
                page.unwrap().read().unwrap().is_frame_s_latched(),
                true
            );
        }

        // Get a page from the pool with X latch
        {
            let resp = buf_pool.create(0, 1, &mut mtr);
            assert!(resp.is_some());

            let page = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::XLatch,
                page_id: PageId { space_id: 0, page_no: 1 },
                mode: BUF_GET_IF_IN_POOL,
                mtr: &mut mtr,
            });
            assert!(page.is_some());
            assert_eq!(
                page.unwrap().read().unwrap().is_frame_x_latched(),
                true
            );
        }
    }

    #[rstest]
    fn test_buf_pool_get_page_multiple_s_latch(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        // Initialize a chunk to have some pages.
        buf_pool.open(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(buf_pool.get_curr_size(), 10 * UNIV_PAGE_SIZE);

        // Get a page from the pool with S latch

        let resp = buf_pool.create(0, 0, &mut mtr);
        assert!(resp.is_some());

        let page = buf_pool.get(Request {
            rw_latch: ReadWriteLatch::SLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_IF_IN_POOL,
            mtr: &mut mtr,
        });
        assert!(page.is_some());
        assert_eq!(page.unwrap().read().unwrap().is_frame_s_latched(), true);

        // calling a second time should not fail.
        let page = buf_pool.get(Request {
            rw_latch: ReadWriteLatch::SLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_IF_IN_POOL,
            mtr: &mut mtr,
        });

        assert!(page.is_some());
        assert_eq!(page.unwrap().read().unwrap().is_frame_s_latched(), true);
    }

    #[rstest]
    fn test_block_remove_hashed_page_scrubs_header_and_updates_clocks(
        buf_pool_fixture: BufPoolFixture,
    ) {
        // 1) Build a fresh pool fixture.
        let pool_arc = buf_pool_fixture.buf_pool.clone();

        let space_id = 0;
        let page_no = 42;
        let owning_page = test0utils::create_raw_page();
        let block = owning_page.buf_block.clone();

        // 4) Initialize the page in the pool (this sets in_file & hashes it).
        {
            let pool = pool_arc.lock().unwrap();
            pool.page_init(
                owning_page.page.page_get_space_id(),
                page_no,
                block.clone(),
            );
        }

        // 5) Add it to LRU as the “young” end.
        {
            let pool = pool_arc.lock().unwrap();
            let mut guard = block.write().unwrap();
            pool.add_block(
                guard.buf_block_get_page_mut(),
                /*old=*/ false,
            );
        }

        // 6) Snapshot clocks & hash‐membership before eviction.
        let (before_pool_clk, before_block_clk, in_hash_before) = {
            let pool = pool_arc.lock().unwrap();
            let pool_clk = pool.freed_page_clock();
            let block_clk = block.read().unwrap().buf_block_get_clock();
            let in_hash = pool.peek(space_id, page_no);
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
            let pool = pool_arc.lock().unwrap();
            let pool_clk = pool.freed_page_clock();
            let block_clk = block.read().unwrap().buf_block_get_clock();
            let in_hash = pool.peek(space_id, page_no);
            (pool_clk, block_clk, in_hash)
        };

        // 10) Verify eviction, clocks, and hash removal.
        assert!(!in_hash_after, "page should no longer be in the hash");
        assert_eq!(
            after_pool_clk,
            before_pool_clk + 1,
            "global clock must +1"
        );
        assert_eq!(
            after_block_clk,
            before_block_clk + 1,
            "block clock must +1"
        );

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

    #[rstest]
    fn test_block_free_moves_memory_page_back_to_free_list_and_resets_state(
        buf_pool_fixture: BufPoolFixture,
    ) {
        // 1) Grab the pool:
        let pool_arc = buf_pool_fixture.buf_pool.clone();

        let space_id = 0;
        let page_no = 0;
        let page_id = PageId { space_id, page_no };
        let owning_page = test0utils::create_raw_page();

        let block = owning_page.buf_block.clone();

        // Manually insert it into the pool’s inner structures as an “in-memory” page:
        {
            let pool = pool_arc.lock().unwrap();

            let mut inner = pool.inner().lock().unwrap();
            inner.page_hash_mut().insert(page_id, block.clone());
            drop(inner);

            // mark it as a memory page
            let mut blk = block.write().unwrap();
            blk.buf_block_set_state(BufPageState::BufBlockMemory);
            // ensure it's not on *any* list yet:
            let pg = blk.buf_block_get_page_mut();
            pg.set_is_in_hash_map(false);
            pg.set_is_in_lru_list(false);
            pg.set_is_in_flush_list(false);
            pg.set_is_in_free_list(false);
            drop(blk);
        }

        // 4) Snapshot state before
        let (before_state, before_in_free) = {
            let pool = pool_arc.lock().unwrap();
            let blk = block.read().unwrap();
            (
                blk.buf_block_get_state(),
                blk.buf_block_get_page().is_in_free_list(),
            )
        };
        assert_eq!(before_state, BufPageState::BufBlockMemory);
        assert!(!before_in_free, "should start out off of the free-list");

        // 5) Call the API under test:
        {
            let mut pool = pool_arc.lock().unwrap();
            pool.block_free(block.clone());
        }

        // 6) Verify post-conditions:
        let (after_state, after_in_free) = {
            let pool = pool_arc.lock().unwrap();
            let blk = block.read().unwrap();
            (
                blk.buf_block_get_state(),
                blk.buf_block_get_page().is_in_free_list(),
            )
        };

        assert_eq!(
            after_state,
            BufPageState::BufBlockNotUsed,
            "block_free must transition to NOT_USED"
        );
        assert!(
            after_in_free,
            "block_free must push the page onto the free list"
        );
    }

    // Test the “new-page” path: when the pool does *not* already
    /// contain (space=1,page=42), create() must allocate, insert,
    /// bump stats, initialize the header, and pin in the MTR.
    #[rstest]
    fn test_create_new_page_initializes_correctly(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let mut mtr = Mtr::new();
        mtr.start();
        assert_eq!(mtr.get_mtr_state(), MtrState::MtrActive);

        let pool = buf_pool_fixture.buf_pool.clone();
        // 0 pages created yet:
        assert_eq!(
            pool.lock().unwrap().stat().n_pages_created.load(Relaxed),
            0
        );
        pool.lock().unwrap().open(10 * UNIV_PAGE_SIZE);

        // 1) call create()
        let block = {
            let mut p = pool.lock().unwrap();
            p.create(1, 42, &mut mtr)
        };
        assert!(block.is_some(), "create should return Some for a free block");
        let block = block.unwrap();

        // 2) after creation, stats.n_pages_created == 1
        assert_eq!(
            pool.lock().unwrap().stat().n_pages_created.load(Relaxed),
            1
        );

        // 3) block must now be in state FilePage, in the pool hash, and in the LRU
        {
            let guard = block.read().unwrap();
            assert_eq!(
                guard.buf_block_get_state(),
                BufPageState::BufBlockFilePage
            );
            let pid = PageId { space_id: 1, page_no: 42 };
            let pool_guard = pool.lock().unwrap();
            let inner = pool_guard.inner().lock().unwrap();
            assert!(
                inner.page_hash().contains_key(&pid),
                "page must be in the hash map"
            );
            assert!(
                guard.buf_block_get_page().is_in_lru_list(),
                "page must have been pushed into LRU"
            );
        }

        // 4) check that we wrote the on-disk header exactly as in InnoDB:
        {
            let guard = block.read().unwrap();
            let page = guard.get_page();
            let frame = page.to_page_frame();
            // prev & next both 0xFFFF_FFFF
            let prev = u32::from_be_bytes(
                frame.0[FIL_PAGE_PREV..FIL_PAGE_PREV + 4].try_into().unwrap(),
            );
            let next = u32::from_be_bytes(
                frame.0[FIL_PAGE_NEXT..FIL_PAGE_NEXT + 4].try_into().unwrap(),
            );
            assert_eq!(prev, 0xFFFF_FFFF);
            assert_eq!(next, 0xFFFF_FFFF);
            // type == Allocated
            let ftype = u16::from_be_bytes(
                frame.0[FIL_PAGE_TYPE..FIL_PAGE_TYPE + 2].try_into().unwrap(),
            );
            assert_eq!(ftype, FilPageType::Allocated as u16);
            // flush_lsn == 0
            let flush_lsn = u64::from_be_bytes(
                frame.0[FIL_PAGE_FILE_FLUSH_LSN..FIL_PAGE_FILE_FLUSH_LSN + 8]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(flush_lsn, 0);
        }

        // 5) and finally, the MTR should have recorded exactly one PageFix slot:
        let memo = mtr.memo();
        assert_eq!(memo.len(), 1);
        assert_eq!(
            memo[0],
            MemoSlot::PageFix(
                PageId { space_id: 1, page_no: 42 },
                FixMode::NoLatch
            )
        );
    }

    /// Test the “already-in-pool” path: second create() for the same
    /// (space,page) must evict the *fresh* block and then return the
    /// *old* block via get(), without bumping n_pages_created.
    #[rstest]
    fn test_create_existing_page_eviction_branch(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let mut mtr = Mtr::new();
        mtr.start();
        let pool = buf_pool_fixture.buf_pool.clone();
        pool.lock().unwrap().open(10 * UNIV_PAGE_SIZE);

        // 1) first create => brand-new
        let b1 = {
            let mut p = pool.lock().unwrap();
            p.create(0, 100, &mut mtr).unwrap()
        };

        // remember how many we’ve created so far
        let before = pool.lock().unwrap().stat().n_pages_created.load(Relaxed);

        // 2) second create(0,100) => should see “already in hash” path
        let b2 = {
            let mut p = pool.lock().unwrap();
            p.create(0, 100, &mut mtr).unwrap()
        };

        // 3) we should get back the *same* block that was in the hash:
        assert!(Arc::ptr_eq(&b1, &b2));

        // 4) n_pages_created should not have increased again
        let after = pool.lock().unwrap().stat().n_pages_created.load(Relaxed);
        assert_eq!(
            after, before,
            "second create should not bump pages_created"
        );

        // 5) and the MTR memo must have two fixes (one for each create/get)
        let memo = mtr.memo();
        assert_eq!(memo.len(), 2);
        // both must be PageFix(0,100,NoLatch)
        assert_eq!(
            memo[0],
            MemoSlot::PageFix(
                PageId { space_id: 0, page_no: 100 },
                FixMode::NoLatch
            )
        );
        assert_eq!(
            memo[1],
            MemoSlot::PageFix(
                PageId { space_id: 0, page_no: 100 },
                FixMode::NoLatch
            )
        );
    }

    #[rstest]
    fn test_block_alloc(buf_pool_fixture: BufPoolFixture) {
        let mut buf_pool = buf_pool_fixture.buf_pool.lock().unwrap();
        let mut mtr = Mtr::new();
        mtr.start();

        buf_pool.open(10 * UNIV_PAGE_SIZE);

        assert_eq!(buf_pool.get_free_list_size(), 10);

        // 1. We should be able to get the first 10 pages by calling block_alloc 10 times.
        for _ in 0..10 {
            let resp = buf_pool.block_alloc();
            assert!(resp.is_some());
        }

        // Any further calls should return none

        let resp = buf_pool.block_alloc();
        assert!(resp.is_none());

        assert_eq!(buf_pool.get_free_list_size(), 0);
    }

    #[rstest]
    fn test_try_free_flushed_blocks_no_work(buf_pool_fixture: BufPoolFixture) {
        let pool = buf_pool_fixture.buf_pool.clone();
        {
            // make sure there's no pending flush work
            let p = pool.lock().unwrap();
            p.inner().lock().unwrap().set_lru_flush_ended(0);
        }

        // should be a no-op
        pool.lock().unwrap().try_free_flushed_blocks();

        // still zero
        let ended =
            pool.lock().unwrap().inner().lock().unwrap().lru_flush_ended();
        assert_eq!(
            ended, 0,
            "with no work pending, it must leave lru_flush_ended at 0"
        );
    }

    #[rstest]
    fn test_try_free_flushed_blocks_clears_pending(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let pool = buf_pool_fixture.buf_pool.clone();
        {
            // seed with some pending work
            let p = pool.lock().unwrap();
            p.inner().lock().unwrap().set_lru_flush_ended(3);
        }

        // should loop and clear that counter
        pool.lock().unwrap().try_free_flushed_blocks();

        let ended =
            pool.lock().unwrap().inner().lock().unwrap().lru_flush_ended();
        assert_eq!(
            ended, 0,
            "try_free_flushed_blocks must consume all pending work (set it to 0)"
        );
    }

    #[rstest]
    fn test_search_and_free_block_empty(buf_pool_fixture: BufPoolFixture) {
        let pool = buf_pool_fixture.buf_pool.clone();
        // no pages at all
        let freed = pool.lock().unwrap().search_and_free_block(0);
        assert!(!freed, "with empty pool, should not free anything");
    }

    #[rstest]
    fn test_search_and_free_block_not_in_lru(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let pool = buf_pool_fixture.buf_pool.clone();
        let page_id = PageId { space_id: 0, page_no: 7 };
        // create a page but do *not* put it in the LRU list:
        let owning = test0utils::create_raw_page();
        let block = owning.buf_block.clone();
        {
            let pool_guard = pool.lock().unwrap();
            // mark it as a file page
            {
                let mut b = block.write().unwrap();
                b.buf_block_set_state(BufPageState::BufBlockFilePage);
                let pg = b.buf_block_get_page_mut();
                pg.set_in_file();
                pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
                pg.set_buf_page_io_fix_count(0);
            }
            // insert into hash so search sees it
            pool_guard
                .inner()
                .lock()
                .unwrap()
                .page_hash_mut()
                .insert(page_id, block.clone());
        }
        // it's not in the LRU list, so should not free
        let freed = pool.lock().unwrap().search_and_free_block(0);
        assert!(!freed, "page not in LRU list must not be evicted");
    }

    #[rstest]
    fn test_search_and_free_block_dirty_page(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let pool = buf_pool_fixture.buf_pool.clone();
        let page_id = PageId { space_id: 0, page_no: 8 };
        let owning = test0utils::create_raw_page();
        let block = owning.buf_block.clone();
        {
            let pool_guard = pool.lock().unwrap();
            // prep the page as file‐page, inserted into both hash and LRU
            {
                let mut b = block.write().unwrap();
                b.buf_block_set_state(BufPageState::BufBlockFilePage);
                let pg = b.buf_block_get_page_mut();
                pg.set_buf_block_ref(Arc::downgrade(&block));
                pg.set_in_file();
                pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
                pg.set_buf_page_io_fix_count(0);
                pg.set_oldest_modification(1);
                pg.set_is_in_flush_list(true)
            }
            pool_guard
                .inner()
                .lock()
                .unwrap()
                .page_hash_mut()
                .insert(page_id, block.clone());
            // push into LRU
            pool_guard.add_block(
                &mut block.write().unwrap().buf_block_get_page_mut(),
                false,
            );
        }
        let freed = pool.lock().unwrap().search_and_free_block(0);
        assert!(!freed, "dirty page must not be evicted");
    }

    #[rstest]
    fn test_search_and_free_block_clean_page_eviction(
        buf_pool_fixture: BufPoolFixture,
    ) {
        let pool = buf_pool_fixture.buf_pool.clone();
        let stats = &pool.lock().unwrap().stat();
        let page_id = PageId { space_id: 0, page_no: 9 };
        let owning = test0utils::create_raw_page();
        let block = owning.buf_block.clone();

        // snapshot clocks
        let (before_pool_clk, before_block_clk) = {
            let p = pool.lock().unwrap();
            let inner = p.inner().lock().unwrap();
            let free_clk = inner.freed_page_clock();
            let block_clk = block.read().unwrap().buf_block_get_clock();
            (free_clk, block_clk)
        };

        {
            let p = pool.lock().unwrap();
            // prep the page as file‐page, inserted into both hash and LRU
            {
                let mut b = block.write().unwrap();
                b.buf_block_set_state(BufPageState::BufBlockFilePage);
                let pg = b.buf_block_get_page_mut();
                pg.set_page_no(9);
                pg.set_space_id(0);
                pg.set_buf_block_ref(Arc::downgrade(&block));
                pg.set_in_file();
                pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
                pg.set_buf_page_io_fix_count(0);
            }
            p.inner()
                .lock()
                .unwrap()
                .page_hash_mut()
                .insert(page_id, block.clone());
            p.add_block(
                &mut block.write().unwrap().buf_block_get_page_mut(),
                false,
            );
        }

        // now call the eviction
        let freed = pool.lock().unwrap().search_and_free_block(0);
        assert!(freed, "clean page must be evicted");

        // verify side effects:
        {
            let p = pool.lock().unwrap();
            let mut inner = p.inner().lock().unwrap();
            assert!(
                !inner.page_hash_mut().contains_key(&page_id),
                "evicted page must be removed from hash"
            );
        }
        {
            let b = block.read().unwrap();
            let pg = b.buf_block_get_page();
            assert!(
                !pg.is_in_lru_list(),
                "evicted page must no longer be in LRU list"
            );
            assert!(
                pg.is_in_free_list(),
                "evicted page must have been pushed onto free list"
            );
            assert_eq!(
                b.buf_block_get_state(),
                BufPageState::BufBlockRemoveHash,
                "block state must be REMOVE_HASH after eviction"
            );
        }
        // clocks must have ticked by exactly one
        let (after_pool_clk, after_block_clk) = {
            let p = pool.lock().unwrap();
            let inner = p.inner().lock().unwrap();
            let free_clk = inner.freed_page_clock();
            let block_clk = block.read().unwrap().buf_block_get_clock();
            (free_clk, block_clk)
        };
        assert_eq!(after_pool_clk, before_pool_clk + 1);
        assert_eq!(after_block_clk, before_block_clk + 1);

        // and our readahead‐eviction counter (since page was “cold”) must have bumped
        assert_eq!(stats.n_ra_pages_evicted.load(Relaxed), 1);
    }
}
