#[cfg(test)]
mod tests {
    use std::{
        ops::Deref,
        sync::{Arc, Mutex},
    };

    use pretty_assertions::assert_eq;
    use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
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
        data::data0types::{DTYPE_INT, DTuple},
        dict::dict0types::{Column, Field, Index},
        fil::{fil0fil::Fil, fil0types::FILL_NULL},
        fsp::{fsp0fsp::Fsp, fsp0types::Direction},
        innodb::innodb0types::PageId,
        lock::lock0lock::LockSys,
        mtr::{
            mtr0mtr::{MemoSlot, Mtr, MtrState::MtrActive},
            mtr0types::FixMode,
        },
        os::{os0io::OsIo, os0runtime::create_runtime},
        page::{
            page0cur::{PAGE_CUR_G, PAGE_CUR_GE, PAGE_CUR_L, PAGE_CUR_LE},
            page0page::{Page, PageDebug},
        },
        rem::rem0rec::Record,
        some_or_panic,
        sync::sync0rw::ReadWriteLatch,
        tests::test0utils::{
            FieldType, create_blocks_in_buf_pool, create_dtuple,
            create_dtuples_from_input_data_chunk,
            fill_block_with_uniform_data,
        },
        ut::{ut0dbg::ut_ad, ut0ut::random_string_with_seed},
    };

    struct BTreeFixture {
        btree: Arc<BTree>,
        index: Arc<Index>,
        buf_pool: Arc<Mutex<BufPool>>,
        lock_sys: Arc<Mutex<LockSys>>,
        page_count: usize,
    }

    #[fixture]
    fn btree_fill_factor() -> usize {
        100
    }

    #[fixture]
    fn fixture(btree_fill_factor: usize) -> BTreeFixture {
        let runtime = create_runtime(1);
        let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
        let fil = Arc::new(Mutex::new(Fil::new(1, os_io, runtime)));
        let buf_rea = Arc::new(Mutex::new(BufRea::new(fil.clone())));
        let buf_lru = Arc::new(Mutex::new(BufLRU::new()));
        let buf_pool = Arc::new(Mutex::new(BufPool::new(
            fil.clone(),
            buf_lru.clone(),
            buf_rea,
        )));
        buf_lru.lock().unwrap().init(Arc::downgrade(&buf_pool));

        let lock_sys = Arc::new(Mutex::new(LockSys::new()));
        let index = Arc::new(Index::new(PageId { space_id: 0, page_no: 0 }));
        let fsp =
            Arc::new(Mutex::new(Fsp::new(buf_pool.clone(), fil.clone())));
        let btree = BTree::new(
            buf_pool.clone(),
            fsp,
            lock_sys.clone(),
            index.clone(),
            btree_fill_factor,
        );
        let page_count = 20;
        let field_name = "test";
        index.add_field(Field {
            col: Arc::new(Mutex::new(Column {
                name: field_name.to_string(),
                index: 0,
                ord_part: 0,
                dtype: DTYPE_INT,
            })),
            name: field_name.to_string(),
            prefix_length: 0,
            fixed_length: 0,
        });
        index.set_n_uniq(1);

        create_blocks_in_buf_pool(buf_pool.clone(), page_count);

        BTreeFixture {
            btree: Arc::new(btree),
            index,
            buf_pool,
            lock_sys,
            page_count,
        }
    }

    #[fixture]
    fn fixture_with_page(fixture: BTreeFixture) -> (BTreeFixture, Page) {
        let btree = fixture.btree.clone();
        let mut mtr = Mtr::new();
        mtr.start();

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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        (fixture, buf_block.read().unwrap().get_page())
    }

    #[fixture]
    fn fixture_with_data(
        fixture: BTreeFixture,
    ) -> (BTreeFixture, Page, Vec<DTuple>) {
        let btree = fixture.btree.clone();
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();
        assert!(page.page_validate());

        btr_cur.position(record, buf_block.clone());

        let low_limit = 100;
        let up_limit = 200;
        let mut input: Vec<[FieldType; 1]> = vec![];
        for i in low_limit..up_limit {
            input.push([FieldType::String(Some(format!(
                "hello world {:05}",
                i
            )))])
        }

        let mut dtuples = create_dtuples_from_input_data_chunk(input);

        for dtuple in dtuples.iter_mut() {
            btr_cur.optimistic_insert(0, dtuple, 0, &mut mtr).unwrap()
        }

        (fixture, page, dtuples)
    }

    #[fixture]
    fn fixture_with_full_page_and_small_data(
        fixture: BTreeFixture,
    ) -> BTreeFixture {
        let seed = 42;
        let mut mtr = Mtr::new();
        mtr.start();

        let btree = fixture.btree.clone();
        let buf_pool = fixture.buf_pool.clone();
        let lock_sys = fixture.lock_sys.clone();

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let buf_block = some_or_panic!(buf_block);
        mtr.memo_push(MemoSlot::PageFix(
            buf_block.read().unwrap().get_page_id(),
            FixMode::Exclusive,
        ));

        btree.page_create(buf_block.clone(), 0, &mut mtr);

        fill_block_with_uniform_data(
            btree.clone(),
            buf_block.clone(),
            buf_pool.clone(),
            lock_sys.clone(),
            seed,
        );

        let page = buf_block.read().unwrap().get_page();
        assert!(page.page_validate());
        assert_eq!(page.page_get_data_size(), 15818);

        fixture
    }

    #[rstest]
    fn test_create_btr_page(fixture: BTreeFixture) {
        let btree = fixture.btree.clone();
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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

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
    fn test_page_set_level(fixture_with_page: (BTreeFixture, Page)) {
        let mut page: Page = fixture_with_page.1;
        let mut mtr = Mtr::new();
        mtr.start();

        let level_max = 16;
        for level in 0..level_max {
            BTree::page_set_level(&mut page, level, &mut mtr);
            let found = BTree::page_get_level(&page);
            assert_eq!(level, found);
        }
    }

    #[rstest]
    fn test_page_set_index_id(fixture_with_page: (BTreeFixture, Page)) {
        let mut page: Page = fixture_with_page.1;
        let seed = 42;
        let mut rng = StdRng::seed_from_u64(seed);

        let values: Vec<usize> =
            (0..10).map(|_| rng.random::<u64>() as usize).collect();
        let mut mtr = Mtr::new();
        mtr.start();

        for index_id in values {
            BTree::page_set_index_id(&mut page, index_id, &mut mtr);
            let found = BTree::page_get_index_id(&page);
            assert_eq!(index_id, found);
        }
    }

    #[rstest]
    fn test_page_tuple_smaller(
        fixture_with_data: (BTreeFixture, Page, Vec<DTuple>),
    ) {
        let mut btr_cur = BTreeCursor::new(
            &fixture_with_data.0.btree,
            fixture_with_data.0.index.clone(),
            fixture_with_data.0.buf_pool.clone(),
            fixture_with_data.0.lock_sys.clone(),
        );

        let page = fixture_with_data.1;

        let mut mtr = Mtr::new();
        mtr.start();

        let buf_block =
            fixture_with_data.0.buf_pool.lock().unwrap().get(Request {
                rw_latch: ReadWriteLatch::XLatch,
                page_id: PageId { space_id: 0, page_no: 0 },
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            });

        ut_ad!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        btr_cur.position(page.page_get_infimum_rec(), buf_block);

        {
            let dtuples = create_dtuples_from_input_data_chunk(vec![[
                FieldType::String(Some(
                    format!("hello world {:05}", 0).to_string(),
                )),
            ]]);

            let dtuple = &dtuples[0];

            assert!(btr_cur.page_tuple_smaller(dtuple, 100));
        }

        {
            let dtuples = create_dtuples_from_input_data_chunk(vec![[
                FieldType::String(Some(
                    format!("hello world {:05}", 1000).to_string(),
                )),
            ]]);

            let dtuple = &dtuples[0];

            assert!(!btr_cur.page_tuple_smaller(dtuple, 100));
        }
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
        let btree = fixture.btree.clone();
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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

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
        let btree = fixture.btree.clone();
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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();
        ut_ad!(page.page_validate());

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
    fn test_pessimistic_insert_in_btree_page() {
        let seed = 1234u64;
        let mut rng = StdRng::seed_from_u64(seed);

        let fixture = fixture(1);
        let btree = fixture.btree.clone();
        let btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );
        let btree = fixture.btree.clone();
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        assert!(page.page_validate());

        btr_cur.position(record, buf_block.clone());
        let limit = 1;

        let step_size = 50;
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

            for (i, dtuple) in dtuples.iter_mut().enumerate() {
                btr_cur.search_to_nth_level(
                    0,
                    dtuple,
                    PAGE_CUR_LE,
                    0,
                    &mut mtr,
                );

                let positioned_block = btr_cur.get_block().unwrap();
                let positioned_page =
                    positioned_block.read().unwrap().get_page();

                assert_eq!(BTree::page_get_level(&positioned_page), 0);

                let result =
                    btr_cur.pessimistic_insert(0, dtuple, 0, &mut mtr);
                assert!(result.is_ok(), "Failed to insert: {:?}", dtuple);
            }
        }

        let smallest_dtuple = create_dtuple(&[FieldType::String(Some(
            "hello world 00000".to_string(),
        ))]);

        btr_cur.search_to_nth_level(
            0,
            &smallest_dtuple,
            PAGE_CUR_LE,
            0,
            &mut mtr,
        );

        let mut records: Vec<Record> = vec![];
        let positioned_block = btr_cur.get_block().unwrap();
        let positioned_page = positioned_block.read().unwrap().get_page();
        let mut curr_page_no = positioned_page.page_get_fil_offset();

        while curr_page_no as u32 != FILL_NULL {
            let current_block =
                fixture.buf_pool.lock().unwrap().get(Request {
                    rw_latch: ReadWriteLatch::NoLatch,
                    page_id: PageId {
                        space_id: positioned_page.page_get_space_id(),
                        page_no: curr_page_no as u32,
                    },
                    mode: BUF_GET_ALWAYS,
                    mtr: &mut mtr,
                });

            let page =
                some_or_panic!(current_block).read().unwrap().get_page();

            let recs = page.page_get_user_records();
            for r in recs.iter() {
                records.push(r.clone());
            }

            curr_page_no = BTree::page_get_next(&page) as usize;
        }

        let mut user_rec_data: Vec<String> = vec![];
        for rec in page.page_get_user_records() {
            user_rec_data.push(rec.rec_get_data(0).unwrap().data_or_panic());
        }

        assert!(user_rec_data.is_sorted());
    }

    #[test]
    fn test_leaves_traversal_left_to_right() {
        let seed = 1234u64;
        let mut rng = StdRng::seed_from_u64(seed);

        let fixture = fixture(1);
        let buf_pool = fixture.buf_pool.clone();
        let btree = fixture.btree.clone();
        let btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );
        let btree = fixture.btree.clone();
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        assert!(page.page_validate());

        btr_cur.position(record, buf_block.clone());
        let limit = 1;

        let step_size = 50;
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

            for (i, dtuple) in dtuples.iter_mut().enumerate() {
                btr_cur.search_to_nth_level(
                    0,
                    dtuple,
                    PAGE_CUR_LE,
                    0,
                    &mut mtr,
                );

                let positioned_block = btr_cur.get_block().unwrap();
                let positioned_page =
                    positioned_block.read().unwrap().get_page();

                assert_eq!(BTree::page_get_level(&positioned_page), 0);

                let result =
                    btr_cur.pessimistic_insert(0, dtuple, 0, &mut mtr);
                assert!(result.is_ok(), "Failed to insert: {:?}", dtuple);
            }
        }

        // Find the left most node
        let dtuple = create_dtuple(&[FieldType::String(Some(
            "hello world 00000".to_string(),
        ))]);

        btr_cur.search_to_nth_level(0, &dtuple, PAGE_CUR_LE, 0, &mut mtr);

        let mut records: Vec<Record> = vec![];

        let positioned_block = btr_cur.get_block().unwrap();
        let positioned_page = positioned_block.read().unwrap().get_page();

        ut_ad!(BTree::page_get_level(&positioned_page) == 0);

        let mut current_page = Some(positioned_page);
        while let Some(page) = current_page {
            let recs = page.page_get_user_records();
            for r in recs.iter() {
                records.push(r.clone());
            }

            let next_page_no = BTree::page_get_next(&page);
            if next_page_no as u32 != FILL_NULL {
                let next_block = buf_pool.lock().unwrap().get(Request {
                    rw_latch: ReadWriteLatch::NoLatch,
                    page_id: PageId {
                        space_id: page.page_get_space_id(),
                        page_no: next_page_no as u32,
                    },
                    mode: BUF_GET_ALWAYS,
                    mtr: &mut mtr,
                });

                let block = some_or_panic!(next_block);
                let next_page = block.read().unwrap().get_page();
                current_page.replace(next_page);
            } else {
                current_page = None;
            }
        }

        let mut user_rec_data: Vec<String> = vec![];
        for rec in page.page_get_user_records() {
            user_rec_data.push(rec.rec_get_data(0).unwrap().data_or_panic());
        }

        assert!(user_rec_data.is_sorted());
    }

    #[test]
    fn test_index_validate() {
        let seed = 1234u64;
        let mut rng = StdRng::seed_from_u64(seed);

        let fixture = fixture(1);
        let buf_pool = fixture.buf_pool.clone();
        let btree = fixture.btree.clone();
        let btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );
        let btree = fixture.btree.clone();
        let mut btr_cur = BTreeCursor::new(
            &fixture.btree,
            fixture.index.clone(),
            fixture.buf_pool.clone(),
            fixture.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        assert!(page.page_validate());

        btr_cur.position(record, buf_block.clone());
        let limit = 1;

        let step_size = 50;
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

            for (i, dtuple) in dtuples.iter_mut().enumerate() {
                btr_cur.search_to_nth_level(
                    0,
                    dtuple,
                    PAGE_CUR_LE,
                    0,
                    &mut mtr,
                );

                let positioned_block = btr_cur.get_block().unwrap();
                let positioned_page =
                    positioned_block.read().unwrap().get_page();

                assert_eq!(BTree::page_get_level(&positioned_page), 0);

                let result =
                    btr_cur.pessimistic_insert(0, dtuple, 0, &mut mtr);
                assert!(result.is_ok(), "Failed to insert: {:?}", dtuple);
            }
        }

        assert!(btree.validate_index());
    }

    #[test]
    fn test_optimistic_insert_in_btree_page() {
        let seed = 1234u64;
        let mut rng = StdRng::seed_from_u64(seed);
        let fixture = fixture(100);
        let btree = fixture.btree.clone();
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
        btree.page_create(buf_block.clone(), 0, &mut mtr);

        let page: Page = buf_block.read().unwrap().get_page();
        let record = page.page_get_infimum_rec();

        assert!(page.page_validate());

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

    #[rstest]
    fn test_page_empty(fixture_with_data: (BTreeFixture, Page, Vec<DTuple>)) {
        let btree = &fixture_with_data.0.btree;
        let page = &fixture_with_data.1;

        // Initially the page has some recs.
        assert_eq!(page.page_get_n_recs(), 100);

        // After page empty call, it should be empty.
        let mut mtr = Mtr::new();
        mtr.start();

        mtr.memo_push(MemoSlot::PageFix(
            PageId { space_id: 0, page_no: 0 },
            FixMode::Exclusive,
        ));

        let buf_block =
            fixture_with_data.0.buf_pool.lock().unwrap().get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id: 0, page_no: 0 },
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            });

        assert!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        btree.page_empty(buf_block.clone(), 10, &mut mtr);

        assert_eq!(page.page_get_n_recs(), 0);
        assert_eq!(BTree::page_get_level(page), 10);

        btree.page_empty(buf_block, 11, &mut mtr);

        assert_eq!(page.page_get_n_recs(), 0);
        assert_eq!(BTree::page_get_level(page), 11);
    }

    #[rstest]
    fn test_page_get_split_rec_with_full_page(
        fixture_with_full_page_and_small_data: BTreeFixture,
    ) {
        let fixture = &fixture_with_full_page_and_small_data;
        let mut mtr = Mtr::new();
        mtr.start();

        let btree = &fixture.btree;
        let buf_pool = &fixture.buf_pool;
        let lock_sys = &fixture.lock_sys;

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let buf_block = some_or_panic!(buf_block);

        let page = buf_block.read().unwrap().get_page();
        assert_eq!(page.page_get_data_size(), 15818);

        let mut btr_cur = BTreeCursor::new(
            btree.deref(),
            fixture.index.clone(),
            buf_pool.clone(),
            lock_sys.clone(),
        );

        btr_cur.position(page.page_get_infimum_rec(), buf_block);

        // New record size same as other rows.
        let dtuple_size = 100;
        let dtuple = create_dtuple(&[FieldType::String(Some(
            random_string_with_seed(dtuple_size, 42),
        ))]);

        let rec = btr_cur.page_get_split_rec(&dtuple, 0);
        assert!(rec.is_some());

        let split_rec_index = page.page_get_rec_index(rec.unwrap());

        let left_sum = page
            .page_get_rec_size_from_start_to_rec(split_rec_index)
            + dtuple_size;

        let right_sum =
            page.page_get_rec_size_from_rec_to_end(split_rec_index);

        assert!(left_sum.abs_diff(right_sum) < 1000);
    }

    #[rstest]
    fn test_page_get_split_rec_with_full_page_at_every_rec(
        fixture_with_full_page_and_small_data: BTreeFixture,
    ) {
        let fixture = &fixture_with_full_page_and_small_data;
        let mut mtr = Mtr::new();
        mtr.start();

        let btree = &fixture.btree;
        let buf_pool = &fixture.buf_pool;
        let lock_sys = &fixture.lock_sys;

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let buf_block = some_or_panic!(buf_block);

        let page = buf_block.read().unwrap().get_page();
        assert_eq!(page.page_get_data_size(), 15818);

        let mut btr_cur = BTreeCursor::new(
            btree.deref(),
            fixture.index.clone(),
            buf_pool.clone(),
            lock_sys.clone(),
        );

        // New record size same as other rows.
        let dtuple_size = 100;
        let dtuple = create_dtuple(&[FieldType::String(Some(
            random_string_with_seed(dtuple_size, 42),
        ))]);

        for rec in page.page_get_user_records() {
            // Position the cursor at every record and call split.
            btr_cur.position(rec, buf_block.clone());

            let rec = btr_cur.page_get_split_rec(&dtuple, 0);
            assert!(rec.is_some());

            let split_rec_index = page.page_get_rec_index(rec.unwrap());

            let left_sum = page
                .page_get_rec_size_from_start_to_rec(split_rec_index)
                + dtuple_size;

            let right_sum =
                page.page_get_rec_size_from_rec_to_end(split_rec_index);

            assert!(left_sum.abs_diff(right_sum) < 1000);
        }
    }

    #[rstest]
    fn test_page_get_split_rec_with_full_page_and_very_large_dtuple(
        fixture_with_full_page_and_small_data: BTreeFixture,
    ) {
        let fixture = &fixture_with_full_page_and_small_data;
        let mut mtr = Mtr::new();
        mtr.start();

        let btree = &fixture.btree;
        let buf_pool = &fixture.buf_pool;
        let lock_sys = &fixture.lock_sys;

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let buf_block = some_or_panic!(buf_block);

        let page = buf_block.read().unwrap().get_page();
        assert_eq!(page.page_get_data_size(), 15818);

        let mut btr_cur = BTreeCursor::new(
            btree.deref(),
            fixture.index.clone(),
            buf_pool.clone(),
            lock_sys.clone(),
        );

        btr_cur.position(page.page_get_infimum_rec(), buf_block);

        let dtuple_size = 9000;
        // New record size same as other rows.
        let dtuple = create_dtuple(&[FieldType::String(Some(
            random_string_with_seed(dtuple_size, 42),
        ))]);

        let rec = btr_cur.page_get_split_rec(&dtuple, 0);
        assert!(rec.is_some());

        let split_rec_index = page.page_get_rec_index(rec.unwrap());

        let left_sum = page
            .page_get_rec_size_from_start_to_rec(split_rec_index)
            + dtuple_size;

        let right_sum =
            page.page_get_rec_size_from_rec_to_end(split_rec_index);

        assert!(left_sum.abs_diff(right_sum) < 1000);
    }

    #[rstest]
    fn test_page_alloc(fixture: BTreeFixture) {
        let buf_pool = &fixture.buf_pool;
        let btree = &fixture.btree;
        let mut mtr = Mtr::new();
        mtr.start();

        for i in 0..19 {
            let buf_block = buf_pool.lock().unwrap().get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id: 0, page_no: i },
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            });

            assert!(buf_block.is_some());

            let buf_block = some_or_panic!(buf_block);

            let page = buf_block.read().unwrap().get_page();
            assert_eq!(page.page_get_n_recs(), 0);

            btree.page_alloc(i, Direction::FspDown, 0, &mut mtr);

            assert_eq!(BTree::page_get_level(&page), 0);
            assert_eq!(
                BTree::page_get_index_id(&page),
                btree.get_index().get_index_id()
            );
        }
    }

    #[rstest]
    fn test_page_create(fixture: BTreeFixture) {
        let buf_pool = &fixture.buf_pool;
        let btree = &fixture.btree;
        let mut mtr = Mtr::new();
        mtr.start();

        for i in 0..20 {
            let buf_block = buf_pool.lock().unwrap().get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id: 0, page_no: i },
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            });

            assert!(buf_block.is_some());

            let buf_block = some_or_panic!(buf_block);

            let page = buf_block.read().unwrap().get_page();
            assert_eq!(page.page_get_n_recs(), 0);

            mtr.memo_push(MemoSlot::PageFix(
                buf_block.read().unwrap().get_page_id(),
                FixMode::Exclusive,
            ));

            btree.page_create(buf_block.clone(), 0, &mut mtr);
        }
    }

    #[rstest]
    fn test_page_get_split_rec_with_empty_page(
        fixture_with_page: (BTreeFixture, Page),
    ) {
        let seed = 42;
        let mut mtr = Mtr::new();
        mtr.start();

        let btree = fixture_with_page.0.btree;
        let buf_pool = fixture_with_page.0.buf_pool;
        let lock_sys = fixture_with_page.0.lock_sys;

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        assert!(buf_block.is_some());

        let buf_block = some_or_panic!(buf_block);

        let page = buf_block.read().unwrap().get_page();
        assert_eq!(page.page_get_n_recs(), 0);

        let mut btr_cur = BTreeCursor::new(
            btree.deref(),
            fixture_with_page.0.index,
            buf_pool,
            lock_sys,
        );

        btr_cur.position(page.page_get_infimum_rec(), buf_block);

        let dtuple = create_dtuple(&[FieldType::String(Some(
            random_string_with_seed(100, seed),
        ))]);

        let rec = btr_cur.page_get_split_rec(&dtuple, 0);
        assert!(rec.is_none());
        assert_eq!(page.page_get_n_recs(), 0);
    }

    /* Start Root Split Operations */
    #[rstest]
    fn test_page_root_split_with_full_page(
        fixture_with_full_page_and_small_data: BTreeFixture,
    ) {
        let fixture = &fixture_with_full_page_and_small_data;
        let mut mtr = Mtr::new();
        mtr.start();

        mtr.memo_push(MemoSlot::PageFix(
            PageId { space_id: 0, page_no: 0 },
            FixMode::Exclusive,
        ));

        let btree = &fixture.btree;
        let buf_pool = &fixture.buf_pool;
        let lock_sys = &fixture.lock_sys;

        let buf_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let buf_block = some_or_panic!(buf_block);

        let root_page = buf_block.read().unwrap().get_page();

        let root_n_recs = root_page.page_get_n_recs();
        let root_recs = root_page
            .page_get_user_records()
            .iter()
            .map(|rec| rec.rec_get_data(0).unwrap().data_or_panic())
            .collect::<Vec<_>>();

        assert_eq!(root_n_recs, 719);
        assert!(root_recs.is_sorted());

        let mut btr_cur = BTreeCursor::new(
            btree.deref(),
            fixture.index.clone(),
            buf_pool.clone(),
            lock_sys.clone(),
        );

        btr_cur.position(root_page.page_get_infimum_rec(), buf_block);

        // New record size same as other rows.
        let dtuple_size = 100;
        let dtuple = create_dtuple(&[FieldType::String(Some(
            random_string_with_seed(dtuple_size, 420),
        ))]);

        btr_cur.root_raise_and_insert(&dtuple, 0, &mut mtr);

        // The first key is the smallest record that points to smaller keys.
        // Second key points to keys than its own value.
        assert_eq!(root_page.page_get_n_recs(), 2);
        let left_most_page =
            some_or_panic!(root_page.page_get_infimum_rec().rec_get_next());

        assert_eq!(
            left_most_page.rec_get_data(0).unwrap().data_or_panic(),
            "UNIVERSAL SMALLEST RECORD"
        );

        let child_page = u32::from_le_bytes(
            left_most_page
                .rec_get_data(1)
                .map(|d| d.data_or_panic().as_bytes().to_vec())
                .unwrap()
                .try_into()
                .unwrap_or_else(|_| panic!("Failed to convert to u32")),
        );
        assert_eq!(child_page, 1);

        let left_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 1 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let left_block = some_or_panic!(left_block);
        let left_page = left_block.read().unwrap().get_page();
        let left_page_recs: Vec<String> = left_page
            .page_get_user_records()
            .iter()
            .map(|rec| rec.rec_get_data(0).unwrap().data_or_panic())
            .collect();

        assert_eq!(left_page.page_get_n_recs(), 349);
        assert!(left_page_recs.is_sorted());

        let right_block = buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: 2 },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        let right_block = some_or_panic!(right_block);
        let right_page = right_block.read().unwrap().get_page();
        let right_page_recs = right_page
            .page_get_user_records()
            .iter()
            .map(|rec| rec.rec_get_data(0).unwrap().data_or_panic())
            .collect::<Vec<_>>();

        assert_eq!(right_page.page_get_n_recs(), 370);
        assert!(right_page_recs.is_sorted());

        assert_eq!(
            root_n_recs,
            left_page.page_get_n_recs() + right_page.page_get_n_recs()
        );

        let root_level = BTree::page_get_level(&root_page);
        let left_page_level = BTree::page_get_level(&left_page);
        let right_page_level = BTree::page_get_level(&right_page);

        assert_eq!(root_level, 1);
        assert_eq!(left_page_level, 0);
        assert_eq!(right_page_level, 0);
    }
    /* End Root Split Operations */
}
