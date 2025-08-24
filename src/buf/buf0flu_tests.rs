#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use intrusive_collections::UnsafeRef;
    use rstest::{fixture, rstest};

    use crate::{
        buf::{
            buf0buf::BufPoolInner,
            buf0dblwr::{
                DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
            },
            buf0flu::{
                BufFlush,
                BufFlushType::{BufFlushList, BufFlushLru},
            },
            buf0lru::BufLRU,
            buf0types::{BufBlock, BufIoFix, BufPageState},
        },
        fil::fil0fil::Fil,
        innodb::innodb0types::{PageNo, SpaceId, UNIV_PAGE_SIZE},
        log::log0log::Log,
        os::{os0io::OsIo, os0runtime::create_runtime},
        tests::test0utils,
    };

    struct BufFluFixture {
        buf_flu: BufFlush,
        fil: Arc<Mutex<Fil>>,
    }

    impl BufFluFixture {
        pub fn new() -> Self {
            tracing::info!(
                "Setting up BufFlush fixture, total double write buffer size: {}",
                DOUBLE_WRITE_BUFFER_SIZE
            );

            let runtime = create_runtime(1);
            let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
            let fil =
                Arc::new(Mutex::new(Fil::new(100, os_io.clone(), runtime)));

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
            let buf_flu = BufFlush::new(dblwr_buf, fil.clone(), redo_log);
            Self { buf_flu, fil }
        }
    }

    /// Helper: push a block’s page to the flush-list tail and mark it dirty & ready.
    fn push_dirty_ready(flusher: &mut BufFlush, blk: &Arc<RwLock<BufBlock>>) {
        let mut g = blk.write().unwrap();
        g.buf_block_set_state(BufPageState::BufBlockFilePage);

        let page = g.buf_block_get_page_mut();
        page.set_in_file();
        page.set_is_in_flush_list(true);
        page.buf_page_set_io_fix(BufIoFix::BufIoNone);
        page.set_buf_page_io_fix_count(0);
        page.set_oldest_modification(8124);
        page.set_newest_modification(10240);

        unsafe {
            flusher.flush_list().push_back(UnsafeRef::from_raw(page));
        }
    }

    /// Helper: push a block’s page to the flush-list tail but make it *unready* (I/O-fixed).
    fn push_unready(
        flusher: &mut crate::buf::buf0flu::BufFlush,
        blk: &Arc<RwLock<BufBlock>>,
    ) {
        let mut g = blk.write().unwrap();
        g.buf_block_set_state(BufPageState::BufBlockFilePage);

        let page = g.buf_block_get_page_mut();
        page.set_in_file();
        page.set_is_in_flush_list(true);
        page.buf_page_set_io_fix(BufIoFix::BufIoWrite); // not ready
        page.set_buf_page_io_fix_count(1);
        page.set_oldest_modification(1);
        page.set_newest_modification(1);

        unsafe {
            flusher.flush_list().push_back(UnsafeRef::from_raw(page));
        }
    }

    #[test]
    fn batch_flush_list_from_tail_up_to_min_n() {
        // Fixture gives us a real BufFlush wired to dblwr/fil/log
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        // We don’t exercise LRU branch here, but batch()’s signature needs these:
        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        // Create three blocks and push to flush-list in order A, B, C (C will be the tail)
        let a = test0utils::create_raw_page().buf_block.clone();
        let b = test0utils::create_raw_page().buf_block.clone();
        let c = test0utils::create_raw_page().buf_block.clone();

        push_dirty_ready(flusher, &a);
        push_dirty_ready(flusher, &b);
        push_dirty_ready(flusher, &c);

        // Ask to flush 2; the implementation walks from the tail → flushes C then B
        let n = flusher.batch(&mut pool, &mut lru, BufFlushList, 2, 0, 0);
        assert_eq!(n, 2, "should flush exactly min_n pages from the tail");

        // Verify IO-fix: C and B got set to Write, A unchanged
        let io_a =
            a.read().unwrap().buf_block_get_page().buf_page_get_io_fix();
        let io_b =
            b.read().unwrap().buf_block_get_page().buf_page_get_io_fix();
        let io_c =
            c.read().unwrap().buf_block_get_page().buf_page_get_io_fix();
        assert_eq!(
            io_a,
            BufIoFix::BufIoNone,
            "head (A) should not be flushed"
        );
        assert_eq!(io_b, BufIoFix::BufIoWrite, "B (tail-1) should be flushed");
        assert_eq!(io_c, BufIoFix::BufIoWrite, "C (tail) should be flushed");
    }

    #[test]
    fn batch_flush_list_skips_unready_but_reaches_target_if_possible() {
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        // Tail order after pushes: A, B(unready), C (C is last pushed → tail)
        let a = test0utils::create_raw_page().buf_block.clone();
        let b = test0utils::create_raw_page().buf_block.clone();
        let c = test0utils::create_raw_page().buf_block.clone();

        push_dirty_ready(flusher, &a);
        push_unready(flusher, &b);
        push_dirty_ready(flusher, &c);

        // min_n=2; even though B is unready, A and C are ready → still reach 2
        let n = flusher.batch(&mut pool, &mut lru, BufFlushList, 2, 0, 0);
        assert_eq!(
            n, 2,
            "should skip unready but still meet min_n with other candidates"
        );

        let io_a =
            a.read().unwrap().buf_block_get_page().buf_page_get_io_fix();
        let io_b =
            b.read().unwrap().buf_block_get_page().buf_page_get_io_fix();
        let io_c =
            c.read().unwrap().buf_block_get_page().buf_page_get_io_fix();

        assert_eq!(io_a, BufIoFix::BufIoWrite, "A should be flushed");
        assert_eq!(
            io_b,
            BufIoFix::BufIoWrite,
            "B was pre I/O-fixed; batch should not (re)flush it"
        );
        assert_eq!(io_c, BufIoFix::BufIoWrite, "C should be flushed");
    }

    #[test]
    fn batch_flush_list_returns_less_when_not_enough_ready() {
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        // Only one ready page out of three
        let a = test0utils::create_raw_page().buf_block.clone();
        let b = test0utils::create_raw_page().buf_block.clone();
        let c = test0utils::create_raw_page().buf_block.clone();

        push_unready(flusher, &a);
        push_unready(flusher, &b);
        push_dirty_ready(flusher, &c);

        let n = flusher.batch(&mut pool, &mut lru, BufFlushList, 2, 0, 0);
        assert_eq!(n, 1, "only one page was ready; batch should return 1");
    }

    #[test]
    fn batch_min_n_zero_and_empty_list() {
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;
        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        // empty list, min_n = 0
        assert_eq!(
            flusher.batch(&mut pool, &mut lru, BufFlushList, 0, 0, 0),
            0
        );

        // empty list, min_n > 0
        assert_eq!(
            flusher.batch(&mut pool, &mut lru, BufFlushList, 3, 0, 0),
            0
        );
    }

    #[test]
    fn batch_flush_list_respects_budget_and_min_n() {
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;
        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        let n_pages = 40;
        let mut blocks = Vec::new();
        for _ in 0..n_pages {
            let blk = test0utils::create_raw_page().buf_block.clone();
            push_dirty_ready(flusher, &blk);
            blocks.push(blk);
        }

        let n = flusher.batch(&mut pool, &mut lru, BufFlushList, 5, 0, 0);
        assert_eq!(n, 5);
    }

    #[test]
    fn batch_lru_flushes_neighbors_until_min_n() {
        // --- 1) Fixture + pool sizing ---
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        let mut pool = BufPoolInner::new();
        // ensure a non-trivial read_ahead_area / scan_len
        pool.set_curr_size(1024);

        // --- 2) Create a real tablespace with a UNIQUE filename ---
        let space_id: SpaceId = 123;
        let table = format!(
            "t_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        {
            let mut fil = fixture.fil.lock().unwrap();
            fil.create_new_single_table_space(space_id, &table, 0, 10_000)
                .expect("failed to create test tablespace");
        }

        // --- 3) Seed LRU tail with dirty, relocatable pages; index in page-hash ---
        let mut lru = BufLRU::new();
        let want = 3usize; // we expect to flush at least this many
        let total = 12usize; // number of pages we seed at the cold end

        for i in 0..total {
            let owning = test0utils::create_raw_page();
            let blk: Arc<RwLock<crate::buf::buf0types::BufBlock>> =
                owning.buf_block.clone();

            {
                // configure page: in-file, (space_id, page_no), dirty, relocatable, "old"
                let mut g = blk.write().unwrap();
                g.buf_block_set_state(BufPageState::BufBlockFilePage);
                let pg = g.buf_block_get_page_mut();

                pg.set_in_file();
                pg.set_space_id(space_id);
                pg.set_page_no(i as PageNo);

                // mark dirty
                pg.set_oldest_modification(100);
                pg.set_newest_modification(200);

                // relocatable (not in I/O and no fixers)
                pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
                pg.set_buf_page_io_fix_count(0);

                // treat as "old" so neighbors are eligible under LRU rules
                pg.set_old(true);

                // put at cold tail
                lru.add_block_to_end_low(pg);
            }

            // index into the page-hash (what try_neighbors() looks up)
            {
                let g = blk.read().unwrap();
                let pid = g.buf_block_get_page().get_page_id();
                pool.page_hash_mut().insert(pid, blk.clone());
            }
        }

        // --- 4) Run LRU batch flush; must reach min_n across neighborhoods ---
        let n = flusher.batch(&mut pool, &mut lru, BufFlushLru, want, 0, 0);
        assert_eq!(
            n, want,
            "LRU branch should flush up to min_n across neighborhoods"
        );
    }

    #[test]
    fn batch_flush_list_does_not_flush_clean_pages() {
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;
        let mut pool = BufPoolInner::new();
        let mut lru = BufLRU::new();

        // Clean page (oldest_modification == 0) but in flush-list
        let blk = test0utils::create_raw_page().buf_block.clone();
        {
            let mut g = blk.write().unwrap();
            g.buf_block_set_state(BufPageState::BufBlockFilePage);
            let p = g.buf_block_get_page_mut();
            p.set_in_file();
            p.set_is_in_flush_list(true);
            p.buf_page_set_io_fix(BufIoFix::BufIoNone);
            p.set_buf_page_io_fix_count(0);
            p.set_oldest_modification(0);
            p.set_newest_modification(0);
            unsafe {
                flusher.flush_list().push_back(UnsafeRef::from_raw(p));
            }
        }

        let n = flusher.batch(&mut pool, &mut lru, BufFlushList, 1, 0, 0);
        assert_eq!(n, 0, "clean page on flush-list must not be flushed");
    }

    #[test]
    fn batch_lru_clamps_neighborhood_to_space_size() {
        use crate::innodb::innodb0types::{PageNo, SpaceId};
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        let mut pool = BufPoolInner::new();
        pool.set_curr_size(1024);
        let mut lru = BufLRU::new();

        let space_id: SpaceId = 999;
        fixture
            .fil
            .lock()
            .unwrap()
            .create_new_single_table_space(
                space_id,
                &format!("t_{}", rand::random::<u64>()),
                0,
                100,
            )
            .expect("create ts"); // only 100 pages

        // Put anchor at page 98 (near end); mark it old & dirty
        let owning = test0utils::create_raw_page();
        let blk = owning.buf_block.clone();
        {
            let mut g = blk.write().unwrap();
            g.buf_block_set_state(BufPageState::BufBlockFilePage);
            let p = g.buf_block_get_page_mut();
            p.set_in_file();
            p.set_space_id(space_id);
            p.set_page_no(98 as PageNo);
            p.set_oldest_modification(11);
            p.set_newest_modification(22);
            p.buf_page_set_io_fix(BufIoFix::BufIoNone);
            p.set_buf_page_io_fix_count(0);
            p.set_old(true);
            lru.add_block_to_end_low(p);
        }
        let pid = blk.read().unwrap().buf_block_get_page().get_page_id();
        pool.page_hash_mut().insert(pid, blk.clone());

        let n = flusher.batch(&mut pool, &mut lru, BufFlushLru, 1, 0, 0);
        assert_eq!(
            n, 1,
            "should flush the anchor without scanning past space end"
        );
    }

    #[ignore]
    #[test]
    fn batch_lru_skips_pages_missing_from_hash() {
        use crate::innodb::innodb0types::{PageNo, SpaceId};
        let mut fixture = BufFluFixture::new();
        let flusher = &mut fixture.buf_flu;

        let mut pool = BufPoolInner::new();
        pool.set_curr_size(1024);
        let mut lru = BufLRU::new();

        let space_id: SpaceId = 321;
        fixture
            .fil
            .lock()
            .unwrap()
            .create_new_single_table_space(
                space_id,
                &format!("t_{}", rand::random::<u64>()),
                0,
                10_000,
            )
            .expect("create ts");

        // Three pages; don't index the middle one in page-hash
        let mut blks = Vec::new();
        for i in 0..3 {
            let owning = test0utils::create_raw_page();
            let blk = owning.buf_block.clone();
            {
                let mut g = blk.write().unwrap();
                g.buf_block_set_state(BufPageState::BufBlockFilePage);
                let p = g.buf_block_get_page_mut();
                p.set_in_file();
                p.set_space_id(space_id);
                p.set_page_no((100 + i) as PageNo);
                p.set_oldest_modification(1);
                p.set_newest_modification(2);
                p.buf_page_set_io_fix(BufIoFix::BufIoNone);
                p.set_buf_page_io_fix_count(0);
                p.set_old(true);
                lru.add_block_to_end_low(p);
            }
            // index 0 and 2 only
            if i != 1 {
                let pid =
                    blk.read().unwrap().buf_block_get_page().get_page_id();
                pool.page_hash_mut().insert(pid, blk.clone());
            }
            blks.push(blk);
        }

        let n = flusher.batch(&mut pool, &mut lru, BufFlushLru, 2, 0, 0);
        assert_eq!(
            n, 2,
            "should reach min_n by skipping the missing-hash page"
        );
    }
    #[fixture]
    fn buf_flu_fixture() -> BufFluFixture {
        BufFluFixture::new()
    }

    #[rstest]
    fn test_buf_flush_list_count(buf_flu_fixture: BufFluFixture) {
        let buf_flu = buf_flu_fixture.buf_flu;
        assert_eq!(buf_flu.get_flush_list_size(), 0);
    }
}
