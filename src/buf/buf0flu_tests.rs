#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    use intrusive_collections::UnsafeRef;
    use rstest::{fixture, rstest};

    // use crate::buf::buf0flu::BufFlushType;
    use crate::buf::buf0flu::BufFlushType::BufFlushList;
    //use crate::innodb::innodb0types::Lsn;
    use crate::tests::test0utils;
    use crate::{
        buf::{
            buf0buf::BufPoolInner,
            buf0dblwr::{
                DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
            },
            buf0flu::BufFlush,
            buf0lru::BufLRU,
            buf0types::{BufBlock, BufIoFix, BufPageState},
        },
        fil::fil0fil::Fil,
        innodb::innodb0types::UNIV_PAGE_SIZE,
        log::log0log::Log,
        os::{os0io::OsIo, os0runtime::create_runtime},
    };

    struct BufFluFixture {
        buf_flu: BufFlush,
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
            Self { buf_flu: BufFlush::new(dblwr_buf, fil, redo_log) }
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
