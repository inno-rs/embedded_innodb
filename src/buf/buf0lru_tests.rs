/// Tests for the buffer pool LRU list management functionality.
///
/// This module contains tests that verify the correct behavior of the LRU (Least Recently Used)
/// list management in the buffer pool. It tests operations like adding pages to the LRU list,
/// moving pages within the list, and ensuring proper page aging and eviction.
#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use crate::tests::test0utils::create_raw_page;
    use crate::{
        buf::{
            buf0buf::BufPoolInner,
            buf0buf_tests::tests::BufPoolFixture,
            buf0lru::BufLRU,
            buf0types::{BufIoFix, BufPageState},
        },
        tests::test0utils,
    };

    /// Creates a new buffer pool fixture for testing.
    ///
    /// This fixture sets up a buffer pool with a default configuration
    /// that can be used for testing LRU list operations.
    ///
    /// # Returns
    /// A configured BufPoolFixture ready for testing
    #[fixture]
    fn buf_pool_fixture() -> BufPoolFixture {
        BufPoolFixture::new()
    }

    /// Tests the make_young operation on buffer pages.
    ///
    /// This test verifies that when a page is marked as "young" (recently used),
    /// it is correctly moved to the front of the LRU list, preventing it from
    /// being evicted soon.
    ///
    /// # Note
    /// This test is currently a stub and needs to be implemented.
    #[rstest]
    fn test_make_young() {
        // TODO: Implement this test to verify make_young functionality
    }

    #[test]
    fn test_block_remove_hashed_page_increments_clocks_and_returns_memory() {
        let mut lru = BufLRU::new();
        let mut inner = BufPoolInner::new();
        let space_id = 0;
        let page_no = 42;
        let owning_page = test0utils::create_raw_page();
        let buf_page = owning_page.page.clone();
        let block = owning_page.buf_block.clone();

        // PHASE A: setup
        {
            let mut g = block.write().unwrap();
            g.buf_block_set_state(BufPageState::BufBlockFilePage);
            let pg = g.buf_block_get_page_mut();

            pg.set_in_file();
            pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
            pg.set_buf_page_io_fix_count(0);
            lru.add_block_low(pg, false);
        }

        let mut _before_i = 0usize;
        let mut _after_i = 0usize;

        let mut _before_b = 0usize;
        let mut _after_b = 0usize;

        // snapshot before
        {
            _before_i = inner.freed_page_clock();
            _before_b = block.read().unwrap().buf_block_get_clock();
        }
        // PHASE B: eviction

        let new_state = {
            let mut g = block.write().unwrap();
            lru.remove_page_from_lru(&mut inner, &mut g)
        };

        // snapshot after
        {
            _after_i = inner.freed_page_clock();
            _after_b = block.read().unwrap().buf_block_get_clock();
        }
        assert_eq!(_after_i - _before_i, 1);
        assert_eq!(_after_b - _before_b, 1);
    }

    #[test]
    fn block_free_non_file_page_moves_to_free_list_and_resets_state() {
        // --- 1) set up LRU + inner pool ---
        let mut lru = BufLRU::new();
        let mut inner = BufPoolInner::new();

        // --- 2) build one BufBlock in READY_FOR_USE state ---
        let space_id = 1;
        let page_no = 2;
        let owning_page = test0utils::create_raw_page();

        let buf_page = owning_page.page.clone();
        let block = owning_page.buf_block.clone();

        // precondition: free_list empty
        assert_eq!(lru.free_list().iter().count(), 0);

        // --- 3) call the helper under a write-guard ---
        {
            let mut g = block.write().unwrap();
            g.buf_block_set_state(BufPageState::BufBlockReadyForUse);
            // sanity: not yet on any list
            assert!(!g.buf_block_get_page().is_in_free_list());
            // now free it
            lru.block_free_non_file_page(&mut inner, &mut g);
            // after:
            //  a) state → NOT_USED
            assert_eq!(g.buf_block_get_state(), BufPageState::BufBlockNotUsed);
            //  b) page flag → in free list
            assert!(g.buf_block_get_page().is_in_free_list());
            //  c) didn’t touch modify_clock
            assert_eq!(g.buf_block_get_clock(), 0);
        }

        // --- 4) check LRU’s free list grew by 1, and points at our page ---
        let entries: Vec<_> =
            lru.free_list().iter().map(|up| up.get_page_id()).collect();
        assert_eq!(entries.len(), 1);

        // --- 5) global freed_page_clock stays at zero (this helper doesn’t bump it) ---
        assert_eq!(inner.freed_page_clock(), 0);
    }

    #[test]
    fn test_recommendation_end_to_end() {
        use std::cmp::min;

        use crate::buf::{
            buf0buf::BufPoolInner,
            buf0lru::BufLRU,
            buf0types::{BufIoFix, BufPageState},
        };

        // 1) create LRU + pool
        let mut lru = BufLRU::new();
        let _inner = BufPoolInner::new();
        let total = 64usize;

        // recompute margins & scan_len
        let read_ahead = min((total / 32).next_power_of_two(), 64);
        let free_block_margin = 5 + read_ahead as usize;
        let extra_margin = free_block_margin / 4 + 100;
        let target = free_block_margin + extra_margin;
        let scan_len = lru.get_free_search_len(total);

        // 2) build & stash real BufBlock Arcs
        let mut holders: Vec<test0utils::OwningPage> =
            Vec::with_capacity(total);
        for i in 0..total {
            let owning = create_raw_page();
            //let block = test0utils::make_buf_block(); // from step 1
            {
                let block = owning.buf_block.clone();
                let mut g = block.write().unwrap(); // getting error here
                g.buf_block_set_state(BufPageState::BufBlockFilePage);
                let pg = g.buf_block_get_page_mut();
                pg.set_in_file();

                // only the last `scan_len` slots can be counted:
                if i >= total - scan_len {
                    if i % 2 == 0 {
                        // even = “replaceable”
                        pg.buf_page_set_io_fix(BufIoFix::BufIoNone);
                        pg.set_buf_page_io_fix_count(0);
                    } else {
                        // odd = “pinned” ⇒ skipped
                        pg.buf_page_set_io_fix(BufIoFix::BufIoWrite);
                        pg.set_buf_page_io_fix_count(1);
                    }
                } else {
                    // everything else pinned, so it never counts
                    pg.buf_page_set_io_fix(BufIoFix::BufIoWrite);
                    pg.set_buf_page_io_fix_count(1);
                }

                // insert into the *intrusive* LRU list
                lru.add_block_to_end_low(pg);
            }
            holders.push(owning);
        }

        // 3) now call the real recommendation()
        let rec = lru.recommendation();

        // 4) compute expected flush count
        let clean_count =
            (total - scan_len..total).filter(|&i| i % 2 == 0).count();
        let expected = if clean_count >= free_block_margin {
            0
        } else {
            target - clean_count
        };

        assert_eq!(
            rec, expected,
            "recommendation = {}, expected {} (clean_count = {})",
            rec, expected, clean_count
        );
    }
}
