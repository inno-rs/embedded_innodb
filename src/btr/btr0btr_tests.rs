#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
    use rstest::{fixture, rstest};
    use tracing::info;

    use crate::{
        btr::{btr0btr::BTree, btr0cur::BTreeCursor},
        buf::{
            buf0buf::BufPool,
            buf0lru::BufLRU,
            buf0rea::BufRea,
            buf0types::{BUF_GET_ALWAYS, Request},
        },
        dict::dict0types::Index,
        fil::fil0fil::Fil,
        fsp::fsp0fsp::Fsp,
        innodb::innodb0types::PageId,
        lock::lock0lock::LockSys,
        mtr::{
            mtr0mtr::{MemoSlot, Mtr, MtrState::MtrActive},
            mtr0types::FixMode,
        },
        os::{os0io::OsIo, os0runtime::create_runtime},
        page::{
            page0cur::{PAGE_CUR_G, PAGE_CUR_GE, PAGE_CUR_L, PAGE_CUR_LE},
            page0page::Page,
        },
        some_or_panic,
        sync::sync0rw::ReadWriteLatch,
        tests::test0utils::{
            FieldType, create_blocks_in_buf_pool,
            create_dtuples_from_input_data_chunk,
        },
        ut::ut0dbg::ut_ad,
    };

    struct BTreeFixture {
        btree: BTree,
        index: Arc<Index>,
        buf_pool: Arc<Mutex<BufPool>>,
        lock_sys: Arc<Mutex<LockSys>>,
    }

    #[fixture]
    fn fixture() -> BTreeFixture {
        let runtime = create_runtime(1);
        let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
        let fil = Arc::new(Mutex::new(Fil::new(1, os_io, runtime)));
        let buf_rea = Arc::new(Mutex::new(BufRea::new(fil.clone())));
        let buf_lru = Arc::new(Mutex::new(BufLRU::new()));
        let buf_pool =
            Arc::new(Mutex::new(BufPool::new(fil, buf_lru.clone(), buf_rea)));
        buf_lru.lock().unwrap().init(Arc::downgrade(&buf_pool));

        let lock_sys = Arc::new(Mutex::new(LockSys::new()));
        let index = Arc::new(Index::new(PageId { space_id: 0, page_no: 0 }));
        let fsp = Arc::new(Mutex::new(Fsp::new(buf_pool.clone())));
        let btree =
            BTree::new(buf_pool.clone(), fsp, lock_sys.clone(), index.clone());

        create_blocks_in_buf_pool(buf_pool.clone(), 1);

        BTreeFixture { btree, index, buf_pool, lock_sys }
    }

    #[rstest]
    fn test_create_btr_page(fixture: BTreeFixture) {
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        let buf_block = fixture.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::XLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        ut_ad!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        mtr.memo_push(MemoSlot::PageFix(
            buf_block.read().unwrap().get_page_id(),
            FixMode::Exclusive,
        ));

        // Initialize the page.
        BTree::page_create(
            buf_block.clone(),
            fixture.index.clone(),
            0,
            &mut mtr,
        );

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        btr_cur.position(record, buf_block.clone());

        let limit = 10;
        let mut input: Vec<[FieldType; 1]> = vec![];
        for i in 0..limit {
            let val = limit - i;
            input.push([FieldType::String(Some(format!("hello world {val}")))])
        }

        let dtuples = create_dtuples_from_input_data_chunk(input);

        for mut dtuple in dtuples {
            btr_cur.optimistic_insert(0, &mut dtuple, 0, &mut mtr).unwrap()
        }

        assert_eq!(page.page_get_n_recs(), limit);
    }

    #[rstest]
    // First element
    #[case(0, PAGE_CUR_L, "infimumr")]
    #[case(0, PAGE_CUR_LE, "hello world 1")]
    #[case(0, PAGE_CUR_GE, "hello world 1")]
    #[case(0, PAGE_CUR_G, "hello world 2")]
    // Second element
    #[case(1, PAGE_CUR_L, "hello world 1")]
    #[case(1, PAGE_CUR_LE, "hello world 2")]
    #[case(1, PAGE_CUR_GE, "hello world 2")]
    #[case(1, PAGE_CUR_G, "hello world 3")]
    // Third element
    #[case(2, PAGE_CUR_L, "hello world 2")]
    #[case(2, PAGE_CUR_LE, "hello world 3")]
    #[case(2, PAGE_CUR_GE, "hello world 3")]
    #[case(2, PAGE_CUR_G, "hello world 4")]
    // Fifth element
    #[case(5, PAGE_CUR_L, "hello world 5")]
    #[case(5, PAGE_CUR_LE, "hello world 6")]
    #[case(5, PAGE_CUR_GE, "hello world 6")]
    #[case(5, PAGE_CUR_G, "hello world 7")]
    // Seventh Element
    #[case(7, PAGE_CUR_L, "hello world 7")]
    #[case(7, PAGE_CUR_LE, "hello world 8")]
    #[case(7, PAGE_CUR_GE, "hello world 8")]
    #[case(7, PAGE_CUR_G, "hello world 9")]
    // Seventh Element
    #[case(8, PAGE_CUR_L, "hello world 8")]
    #[case(8, PAGE_CUR_LE, "hello world 9")]
    #[case(8, PAGE_CUR_GE, "hello world 9")]
    #[case(8, PAGE_CUR_G, "supremumr")]
    fn test_search_in_btree_page(
        fixture: BTreeFixture,
        #[case] dtuple_to_find: usize,
        #[case] comparison_mode: usize,
        #[case] expected: String,
    ) {
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        let buf_block = fixture.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        ut_ad!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        mtr.memo_push(MemoSlot::PageFix(
            buf_block.read().unwrap().get_page_id(),
            FixMode::Exclusive,
        ));

        // Initialize the page.
        BTree::page_create(
            buf_block.clone(),
            fixture.index.clone(),
            0,
            &mut mtr,
        );

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        btr_cur.position(record, buf_block.clone());

        let limit = 9;
        let mut input: Vec<[FieldType; 1]> = vec![];
        for i in 0..limit {
            let val = limit - i;
            input.push([FieldType::String(Some(format!("hello world {val}")))])
        }

        let mut dtuples = create_dtuples_from_input_data_chunk(input);

        for dtuple in dtuples.iter_mut() {
            btr_cur.optimistic_insert(0, dtuple, 0, &mut mtr).unwrap()
        }

        dtuples.reverse();

        assert_eq!(page.page_get_n_recs(), limit);

        info!("finding: {}", &dtuples[dtuple_to_find]);

        btr_cur.search_to_nth_level(
            vec![],
            0,
            &dtuples[dtuple_to_find],
            comparison_mode,
            0,
            &mut mtr,
        );

        let rec = btr_cur.get_rec();
        assert!(rec.is_some());
        let rec = some_or_panic!(rec);

        let record_field = some_or_panic!(rec.rec_get_data(0));
        let data = record_field.data_or_panic();

        assert_eq!(data, expected);
    }

    #[rstest]
    fn test_optimistic_insert_returns_none(fixture: BTreeFixture) {
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        let buf_block = fixture.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        ut_ad!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        mtr.memo_push(MemoSlot::PageFix(
            buf_block.read().unwrap().get_page_id(),
            FixMode::Exclusive,
        ));

        // Initialize the page.
        BTree::page_create(
            buf_block.clone(),
            fixture.index.clone(),
            0,
            &mut mtr,
        );

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        btr_cur.position(record, buf_block.clone());

        let limit = 750;
        for i in 0..limit {
            let mut dtuples = create_dtuples_from_input_data_chunk(vec![[
                FieldType::String(Some(format!("hello world {i}"))),
            ]]);

            for (n, dtuple) in dtuples.iter_mut().enumerate() {
                let ret = btr_cur.optimistic_insert(0, dtuple, 0, &mut mtr);

                if i < 724 {
                    assert!(ret.is_ok());
                } else {
                    assert!(ret.is_err());
                }
            }
        }
    }

    #[test]
    fn test_optimistic_insert_in_btree_page() {
        let seed = 1234u64;
        let mut rng = StdRng::seed_from_u64(seed);
        let fixture = fixture();
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MtrActive);

        let buf_block = fixture.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        ut_ad!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        mtr.memo_push(MemoSlot::PageFix(
            buf_block.read().unwrap().get_page_id(),
            FixMode::Exclusive,
        ));

        // Initialize the page.
        BTree::page_create(
            buf_block.clone(),
            fixture.index.clone(),
            0,
            &mut mtr,
        );

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        btr_cur.position(record, buf_block.clone());
        let limit = 150;
        let step_size = 20;
        for chunk in (0..limit).step_by(step_size) {
            let mut input: Vec<[FieldType; 1]> = vec![];

            for c in chunk..chunk + step_size {
                input.push([FieldType::String(Some(format!(
                    "hello world {:05}",
                    c
                )))])
            }

            let mut dtuples = create_dtuples_from_input_data_chunk(input);

            dtuples.shuffle(&mut rng);

            for dtuple in dtuples.iter_mut() {
                btr_cur.search_to_nth_level(
                    vec![],
                    0,
                    dtuple,
                    PAGE_CUR_LE,
                    0,
                    &mut mtr,
                );
                btr_cur.optimistic_insert(0, dtuple, 0, &mut mtr).unwrap()
            }
        }

        let mut user_rec_data: Vec<String> = vec![];
        for rec in page.page_get_user_records() {
            user_rec_data.push(rec.rec_get_data(0).unwrap().data_or_panic());
        }

        assert!(user_rec_data.is_sorted());
    }
}
