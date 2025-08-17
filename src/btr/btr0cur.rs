use std::{
    cmp::Ordering,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use tracing::info;

use crate::{
    btr::{
        btr0btr::BTree,
        btr0types::{BTR_ESTIMATE, BtrLatchMode::BtrContModifyTree},
    },
    buf::{
        buf0buf::BufPool,
        buf0types::{BUF_GET_ALWAYS, BufBlock, Request},
    },
    data::data0types::DTuple,
    dict::dict0types::Index,
    fil::fil0types::FILL_NULL,
    fsp::fsp0types::Direction,
    innodb::innodb0types::{
        DbErr,
        DbErr::{DbFail, DbTooBigRecord},
        PageId, PageNo,
    },
    lock::lock0lock::LockSys,
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{FixMode, MemoSlot},
    },
    page::{
        page0cur::{MatchCache, PAGE_CUR_L, PAGE_CUR_LE, PageCur},
        page0page::Page,
    },
    rem::{rem0cmp::cmp_dtuple_rec, rem0rec::Record},
    some_or_panic,
    sync::sync0rw::ReadWriteLatch,
    trx::trx0trx::Trx,
    ut::ut0dbg::ut_ad,
    ut_panic,
};

/// Flag indicating that undo logging should be skipped for this operation.
pub const BTR_NO_UNDO_LOG: usize = 1;

/// Flag indicating that record lock checking should be skipped for this operation.
pub const BTR_NO_LOCKING_FLAG: usize = 2;

/// Flag indicating that system fields should be preserved during the operation.
pub const BTR_KEEP_SYS_FLAG: usize = 4;

/// Represents a path in the B-tree traversal.
///
/// This struct keeps track of the position within a page during B-tree traversal.
pub struct Path {
    /// Index of the record where the page cursor stopped on this level (index in alphabetical order).
    /// usize::MAX denotes the end.
    pub nth_rec: usize,
    /// Number of records on the page.
    pub n_recs: usize,
    /// Page of the page containing the record.
    pub page_no: PageNo,
    /// Level of the page, if later we fetch the page with page_no, and its page_no is not different, then we know that the page not been re-organized.
    pub page_level: usize,
}

/// A cursor for navigating and manipulating B-tree structures.
///
/// BTreeCursor provides methods for traversing, searching, and modifying records
/// within a B-tree index structure. It maintains the current position within the
/// B-tree and provides operations relative to that position.
pub struct BTreeCursor<'a> {
    /// Height of the tree.
    tree_height: usize,
    /// Reference to the B-tree being traversed.
    btree: &'a BTree,
    /// Current page cursor position.
    page_cur: Option<PageCur>,
    /// The current buffer block being accessed.
    buf_block: Option<Arc<RwLock<BufBlock>>>,
    /// Reference to the index,
    index: Arc<Index>,
    /// Reference to the buffer pool.
    buf_pool: Arc<BufPool>,
    /// Reference to LockSys.
    lock_sys: Arc<Mutex<LockSys>>,
    /// Path vector to remember the path to the current page from root.
    paths: Vec<Path>,
}

impl<'a> BTreeCursor<'a> {
    /// Creates a new BTreeCursor instance.
    ///
    /// # Arguments
    /// * `index` - A reference to the index being accessed
    /// * `fsp` - A reference to the file space manager
    /// * `btree` - A reference to the B-tree being traversed
    ///
    /// # Returns
    /// A new BTreeCursor instance initialized with the provided references
    pub fn new(
        btree: &'a BTree,
        index: Arc<Index>,
        buf_pool: Arc<BufPool>,
        lock_sys: Arc<Mutex<LockSys>>,
    ) -> Self {
        BTreeCursor {
            tree_height: 0,
            btree,
            page_cur: None,
            buf_block: None,
            index,
            buf_pool,
            lock_sys,
            paths: vec![],
        }
    }

    /// Determines if a page is a leaf page in the B-tree.
    ///
    /// Leaf pages contain actual data records, while non-leaf pages contain
    /// pointers to other pages.
    ///
    /// # Arguments
    /// * `page` - The page to check
    ///
    /// # Returns
    /// `true` if the page is a leaf page, `false` otherwise
    pub fn page_is_leaf(page: Page) -> bool {
        // A page is a leaf if it has no children, i.e., it is at the lowest level of the B-tree.
        // In InnoDB, leaf pages are identified by having a level of 0.
        BTree::page_get_level(&page) == 0
    }

    /// Tries to perform an optimistic insert to a page in an index tree.
    ///
    /// This method attempts to insert a record next to the cursor position.
    /// It is assumed
    /// that the mini-transaction (Mtr) holds an exclusive latch (x-latch) on the page.
    /// The operation will not succeed if there is not enough space on the page.
    /// If there is only one record on the page, the operation will always succeed.
    ///
    /// # Arguments
    /// * `flags` - Flags controlling the insert operation
    /// * `dtuple` - The data tuple to be inserted
    /// * `n_ext` - Number of externally stored fields
    /// * `mtr` - Mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// * `Ok(usize)` - Success, with a status code
    /// * `Err(DbErr)` - Failure, with the specific error that occurred
    pub fn optimistic_insert(
        &mut self,
        flags: usize,
        dtuple: &mut DTuple,
        n_ext: usize,
        mtr: &mut Mtr,
    ) -> Result<(), DbErr> {
        let buf_block = some_or_panic!(self.buf_block.clone());

        let (page, page_id) = {
            let buf_block_guard = buf_block.read().unwrap();
            (buf_block_guard.get_page(), buf_block_guard.get_page_id())
        };

        // ut_ad!(
        //     mtr.memo_contains(&MemoSlot::PageFix(page_id, FixMode::Exclusive)),
        //     "page={} should be fixed in the mini-transaction",
        //     page_id
        // );

        // let max_size = page.page_get_max_insert_size_after_reorganize(1);
        let mut rec_size = dtuple.rec_get_converted_size(n_ext);

        if Page::page_rec_needs_ext(rec_size) {
            // Move the big record out of dtuple.
            let big_rec = dtuple.dtuple_convert_big_rec(n_ext);
            match big_rec {
                None => return Err(DbTooBigRecord),
                Some(_) => {
                    // Recalculate the record size after moving external fields.
                    rec_size = dtuple.rec_get_converted_size(n_ext);
                }
            }
        }

        let is_clustered = self.index.is_clustered();

        info!(
            "fill_factor: {}, rec_size: {}, page size: {}, current fill factor: {}",
            self.btree.get_fill_factor(),
            rec_size,
            page.page_get_data_size(),
            page.page_get_current_fill_factor()
        );

        // If there have been many consecutive inserts on the page, and we are on the leaf, check if we have to split the page to reserve enough free space for future updates of records.
        if is_clustered
            && page.page_n_get_recs() >= 2
            && Self::page_is_leaf(page)
            && (page.page_get_max_insert_size(1) < rec_size
                || page.page_get_current_fill_factor()
                    >= self.btree.get_fill_factor())
        {
            return Err(DbFail(format!(
                "Optimistic insert failed: too many records on the page, btree_fill_factor={}, current_fill_factor={}",
                self.btree.get_fill_factor(),
                page.page_get_current_fill_factor()
            )));
        }

        if is_clustered
            && page.page_n_get_recs() >= 2
            && (page.page_get_max_insert_size(1) < rec_size
                || page.page_get_current_fill_factor()
                    >= self.btree.get_fill_factor())
        {
            return Err(DbFail(format!(
                "Optimistic insert failed: too many records on the page, btree_fill_factor={}, current_fill_factor={}",
                self.btree.get_fill_factor(),
                page.page_get_current_fill_factor()
            )));
        }

        let mut insert_rec = Option::<Record>::None;
        // Try inserting the record
        match &self.page_cur {
            None => {
                ut_ad!(false, "page cursor should not be None");
            }
            Some(cur) => {
                insert_rec = cur.page_cur_tuple_insert(
                    dtuple,
                    self.index.deref(),
                    n_ext,
                    mtr,
                );
            }
        }

        let reorg = false;

        if insert_rec.is_none() && !reorg {
            let success = self.page_reorganize(mtr);
        }

        if insert_rec.is_none() {
            return Err(DbFail("no space left on page".to_string()));
        }

        Ok(())
    }

    /// Performs a pessimistic insert operation in the B-tree.
    ///
    /// This method first attempts an optimistic insert. If that fails, it proceeds
    /// with a pessimistic approach that may involve page splits or reorganization
    /// to ensure the record can be inserted.
    ///
    /// # Arguments
    ///
    /// * `flags` - Operation flags (BTR_NO_UNDO_LOG, BTR_NO_LOCKING_FLAG, etc.)
    /// * `dtuple` - The data tuple to insert
    /// * `n_ext` - Number of externally stored fields
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// * `Ok(usize)` - Success with the inserted record position
    /// * `Err(DbErr)` - Error code if the operation failed
    pub fn pessimistic_insert(
        &mut self,
        flags: usize,
        dtuple: &mut DTuple,
        n_ext: usize,
        mtr: &mut Mtr,
    ) -> Result<(), DbErr> {
        ut_ad!(dtuple.dtuple_type_checked());

        // 1. First, try an optimistic insert.
        let result = self.optimistic_insert(flags, dtuple, n_ext, mtr);
        match result {
            Ok(_) => return Ok(()),
            Err(e) if !matches!(e, DbFail(_)) => return Err(e),
            Err(_) => {}
        }

        // 2. We were not able to insert. Try pessimistic insert.
        if self.index.page_id().page_no
            == some_or_panic!(&self.buf_block)
                .read()
                .unwrap()
                .get_page_id()
                .page_no
        {
            self.root_raise_and_insert(dtuple, n_ext, mtr);
        } else {
            self.page_split_and_insert(dtuple, n_ext, mtr);
        }

        Ok(())
    }

    /// Retrieves the buffer block associated with the current cursor position.
    ///
    /// This method returns a reference to the buffer block where the cursor is currently
    /// positioned. The buffer block contains the page data and associated metadata.
    ///
    /// # Returns
    ///
    /// * `Some(Arc<RwLock<BufBlock>>)` - A reference to the buffer block if the cursor is positioned
    /// * `None` - If the cursor is not associated with any buffer block
    pub fn get_block(&self) -> Option<Arc<RwLock<BufBlock>>> {
        self.buf_block.clone()
    }

    /// Retrieves the current record at the cursor position.
    ///
    /// # Returns
    ///
    /// * `Some(Record)` - The record at the current cursor position
    /// * `None` - If the cursor is not positioned on any record
    pub fn get_rec(&self) -> Option<Record> {
        match &self.page_cur {
            None => None,
            Some(cur) => Some(cur.page_cur_get_rec()),
        }
    }

    /// Checks locks and performs undo logging for an insert operation.
    ///
    /// This method verifies that the current transaction has the necessary
    /// locks to insert a record at the current position. It also handles
    /// undo logging if required by the operation flags.
    ///
    /// # Arguments
    ///
    /// * `flags` - Operation flags that control locking and undo logging behavior
    /// * `dtuple` - The data tuple being inserted
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// * `()` - If the operation can proceed
    /// * `DbErr` - If there are locking conflicts or other issues
    pub fn ins_lock_and_undo(
        &self,
        flags: usize,
        dtuple: &DTuple,
        trx: Arc<Trx>,
        mtr: &mut Mtr,
    ) -> Result<(), DbErr> {
        let rec = self.get_rec().unwrap();

        let mut lock_sys = self.lock_sys.lock().unwrap();
        let buf_block = self.buf_block.as_ref().unwrap();

        let ret = lock_sys.rec_insert_check_and_lock(
            flags,
            rec,
            buf_block.clone(),
            self.index.clone(),
            trx,
            mtr,
        );

        ret
    }

    /// Reorganizes a page to create more free space.
    ///
    /// This method rearranges the records on a page to eliminate fragmentation
    /// and maximize the available free space. This is typically called when
    /// an insert operation fails due to insufficient space.
    ///
    /// # Arguments
    /// * `mtr` - Mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// `true` if the reorganization was successful, `false` otherwise
    pub fn page_reorganize(&self, mtr: &mut Mtr) -> bool {
        let buf_block = some_or_panic!(&self.buf_block).clone();
        self.btree.page_reorganize(buf_block, mtr)
    }

    /// Searches the B-tree down to the specified level.
    ///
    /// This method traverses the B-tree index structure from the root down to the
    /// specified level, searching for the position where the given data tuple would
    /// be located. It can be used for both read and write operations, with different
    /// latching strategies based on the operation type.
    ///
    /// # Arguments
    ///
    /// * `paths` - Vector to store the traversal path information
    /// * `index` - The index being searched
    /// * `level` - The target level to reach (0 = leaf level)
    /// * `dtuple` - The data tuple to search for
    /// * `mode` - Search mode (exact match, greater than, etc.)
    /// * `latch_mode` - Latching strategy to use during traversal
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    pub fn search_to_nth_level(
        &mut self,
        level: usize,
        dtuple: &DTuple,
        mode: usize,
        latch_mode: usize,
        mtr: &mut Mtr,
    ) {
        // Verify that the search tuple is valid for this index
        // ut_ad!(self.index.check_search_tuple(dtuple));
        ut_ad!(dtuple.dtuple_type_checked());

        // Track the height of the tree as we traverse it
        let mut height: Option<usize> = None;
        let latch = ReadWriteLatch::SLatch;

        // Check if we're in estimation mode
        let estimate = latch_mode & BTR_ESTIMATE;

        let page_cursor = self.page_cur.as_mut().unwrap();

        // Get the root page ID from the index
        let PageId { space_id, mut page_no } = self.index.get_page_id();

        // Initialize a cache for matching records
        let mut match_cache: MatchCache = Default::default();

        // Search loop - continues until we find the target position
        let mut found = false;
        while found == false {
            // Get the buffer block for the current page
            let block = self.buf_pool.get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id, page_no },
                mode: BUF_GET_ALWAYS,
                mtr,
            });

            if block.is_none() {
                // Retry getting the block again if the request failed
                continue;
            }

            let block = some_or_panic!(block);
            let page = block.read().unwrap().get_page();
            ut_ad!(page.page_validate());

            // Verify that the page belongs to the correct index
            ut_ad!(
                BTree::page_get_index_id(&page) == self.index.get_index_id()
            );

            let page_level = BTree::page_get_level(&page);

            if height.is_none() {
                // We are in the root node, initialize the tree height
                self.tree_height = page_level + 1;
            }

            height.replace(page_level);

            let height_val = some_or_panic!(height);
            if height_val == level {
                // We have reached the leaf level
                if latch == ReadWriteLatch::NoLatch {
                    // Unlatch the page, and then return.
                }

                found = true;
            }

            // Create a page cursor for the current page
            let mut page_cur = PageCur::new(None, block.clone());

            // Search for the position of the data tuple in the current page
            page_cur.page_cur_search_with_match(
                dtuple,
                mode,
                &mut match_cache,
            );

            // Position the cursor at the above record.
            self.position(page_cur.page_cur_get_rec(), block);

            if found {
                // If we found the record, we can exit the loop
                break;
            }

            // go to the child page
            let rec = page_cur.page_cur_get_rec();
            page_no = self.btree.inner_node_get_child_page_no(&rec);
        }
    }

    /// Positions the cursor at a specific record in the B-tree.
    ///
    /// This method updates the cursor's internal state to point to the given record
    /// and buffer block. It creates a new page cursor at the specified record position.
    ///
    /// # Arguments
    ///
    /// * `rec` - The record to position the cursor at
    /// * `buf_block` - The buffer block containing the record
    pub fn position(&mut self, rec: Record, buf_block: Arc<RwLock<BufBlock>>) {
        let page = buf_block.read().unwrap().get_page();
        ut_ad!(page.is_page_record(rec));

        self.buf_block = Some(buf_block.clone());
        self.page_cur = Some(PageCur::new(Some(rec), buf_block.clone()))
    }

    /// Calculates a split record such that the tuple will certainly fit on its half-page when the split is performed.
    ///
    /// # Arguments
    ///
    /// `dtuple`: Data tuple to be inserted.
    /// `n_ext`: Number of external fields.
    ///
    /// # Returns
    ///
    /// None, if the data tuple will be the first record on the page.
    /// Otherwise, the record to be split.
    pub fn page_get_split_rec(
        &self,
        dtuple: &DTuple,
        n_ext: usize,
    ) -> Option<Record> {
        let page = some_or_panic!(&self.buf_block).read().unwrap().get_page();
        // This is the location where we are planning to insert the record.
        // This is according to the sort order.
        let insert_rec = some_or_panic!(&self.page_cur).page_cur_get_rec();

        let insert_size = dtuple.rec_get_converted_size(n_ext);
        let free_space = Page::page_get_free_space_of_empty();

        let total_data = insert_size + page.page_get_data_size();
        let total_n_recs = page.page_get_n_recs() + 1;

        let total_space =
            total_data + Page::page_dir_calc_reserved_space(total_n_recs);

        let mut incl_data = 0;
        let mut rec = Some(page.page_get_infimum_rec());

        // We linearly keep on including records and keep on checking if the
        // total accumulated size is less than total_space / 2.
        let mut dtuple_included = false;
        while (incl_data + Page::page_dir_calc_reserved_space(total_n_recs)
            < total_space / 2)
            || (page.page_get_fill_factor_after_insert(
                incl_data + Page::page_dir_calc_reserved_space(total_n_recs),
            ) < self.btree.get_fill_factor())
        {
            if let Some(record) = rec {
                info!("rec: {}", record);
                if record == insert_rec && dtuple_included == false {
                    incl_data += insert_size;
                    dtuple_included = true;
                } else if page.page_rec_is_user_rec(record) {
                    incl_data += record.rec_offs_size();
                    rec = record.rec_get_next();
                } else {
                    rec = record.rec_get_next();
                }
            }

            if rec.is_none() {
                break;
            }
        }

        if let Some(split_rec) = rec
            && incl_data + insert_size < free_space
        {
            return if split_rec == insert_rec
                && let Some(rec) = split_rec.rec_get_next()
                && page.page_rec_is_supremum(rec) == false
            {
                Some(rec)
            } else {
                Some(split_rec)
            };
        }

        rec
    }

    /// Check if the given tuple is smaller than every record on the page.
    pub fn page_tuple_smaller(&self, dtuple: &DTuple, n_uniq: usize) -> bool {
        let mut page_cur = self.page_cur.clone().unwrap();

        page_cur.page_cur_set_before_first();
        page_cur.page_cur_move_to_next();

        let first_rec = page_cur.page_cur_get_rec();
        ut_ad!(
            self.buf_block
                .clone()
                .unwrap()
                .read()
                .unwrap()
                .get_page()
                .page_rec_is_user_rec(first_rec)
        );

        cmp_dtuple_rec(dtuple, first_rec) == Ordering::Less
    }

    /// Splits an index page to two halves and inserts the tuple.
    ///
    /// This function handles the process of splitting a B-tree index page when it becomes too full
    /// to accommodate a new record. It determines the optimal split point, allocates a new page,
    /// and redistributes records between the original and new pages.
    ///
    /// # Arguments
    /// * `dtuple` - The data tuple to be inserted
    /// * `n_ext` - The number of externally stored fields in the tuple
    /// * `mtr` - The mini-transaction that logs this operation
    ///
    /// # Note
    /// This function is part of the B-tree page management system and is crucial for maintaining
    /// the balance and performance of the B-tree index structure.
    pub fn page_split_and_insert(
        &mut self,
        dtuple: &DTuple,
        n_ext: usize,
        mtr: &mut Mtr,
    ) {
        let split_rec = self.page_get_split_rec(dtuple, n_ext);
        let buf_block = some_or_panic!(self.buf_block.clone());

        let page = self.page_cur.as_ref().unwrap().page_cur_get_page();

        let level = BTree::page_get_level(&page);

        let hint_page_no: PageNo = 1;
        let direction = Direction::FspDown;

        let new_block =
            self.btree.page_alloc(hint_page_no, direction, level, mtr);

        if let Some(new_block) = new_block {
            mtr.memo_push(MemoSlot::PageFix(
                new_block.read().unwrap().get_page_id(),
                FixMode::Exclusive,
            ));

            self.btree.page_create(new_block.clone(), level, mtr);

            page.page_move_rec_list_end(
                new_block.clone(),
                some_or_panic!(split_rec),
                self.index.clone(),
                mtr,
            );

            self.attach_half_pages(
                some_or_panic!(split_rec),
                buf_block,
                new_block,
                direction,
                mtr,
            );
        }
    }

    /// Raises the root level of the B-tree and inserts a new record.
    ///
    /// This function is called when the root page of a B-tree needs to be raised to a higher level,
    /// typically during a page split operation. It allocates a new page at the same level as the
    /// current root, and prepares for the insertion of a new record.
    ///
    /// # Arguments
    /// * `page_no` - The page number of the page being operated on
    /// * `root` - The current root page of the B-tree
    /// * `dtuple` - The data tuple to be inserted
    /// * `n_ext` - The number of externally stored fields
    /// * `mtr` - The mini-transaction that logs this operation
    pub fn root_raise_and_insert(
        &mut self,
        dtuple: &DTuple,
        n_ext: usize,
        mtr: &mut Mtr,
    ) {
        let PageId { space_id: root_space_id, page_no: root_page_no } =
            self.index.get_page_id();
        let root_block = some_or_panic!(&self.buf_block);

        let mut root_page =
            self.page_cur.as_ref().unwrap().page_cur_get_page();

        let space_id = root_space_id;
        let page_no = root_page_no;

        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix(
            PageId { space_id, page_no },
            FixMode::Exclusive,
        )));

        // ut_ad!(mtr.memo_contains(&MemoSlot::Latch(
        //     LatchHandle(),
        //     LockMode::Exclusive,
        // )));

        let level = BTree::page_get_level(&root_page);
        BTree::page_set_level(&mut root_page, level + 1, mtr);

        // 1. Create a new page at the same level as the root.
        let buf_block =
            self.btree.page_alloc(0, Direction::FspDown, level, mtr);

        let dest_block = some_or_panic!(buf_block);
        let dest_page_id = dest_block.read().unwrap().get_page_id();
        let mut dest_page = dest_block.read().unwrap().get_page();

        // Initialize the page
        Page::page_create(
            self.index.deref(),
            dest_block.read().unwrap().deref(),
            Some(mtr),
        );

        // Set the index ID for the page
        BTree::page_set_index_id(
            &mut dest_page,
            self.index.get_index_id(),
            mtr,
        );

        // Set the level of the page
        BTree::page_set_level(&mut dest_page, level, mtr);

        ut_ad!(BTree::page_get_level(&dest_page) == level);

        // 2. Move records from the root to the new page.
        ut_ad!(root_page.page_get_n_recs() > 0);

        root_page.page_move_rec_list_end(
            dest_block.clone(),
            root_page.page_get_infimum_rec(),
            self.index.clone(),
            mtr,
        );

        ut_ad!(root_page.page_get_n_recs() == 0);

        BTree::page_set_next(
            &mut dest_page,
            BTree::page_get_next(&root_page),
            mtr,
        );
        BTree::page_set_prev(
            &mut dest_page,
            BTree::page_get_prev(&root_page),
            mtr,
        );

        // 3. Rebuild the root page to get free space.
        self.btree.page_empty(root_block.clone(), level + 1, mtr);

        // 4. Create the smallest record on the new root page and make it point to the new page.
        let smallest =
            self.index.build_smallest_inner_node(dest_page_id.page_no);

        let root_page_cur = PageCur::new(
            Some(root_page.page_get_infimum_rec()),
            some_or_panic!(&self.buf_block).clone(),
        );

        root_page_cur.page_cur_tuple_insert(
            &smallest,
            self.index.deref(),
            0,
            mtr,
        );

        // Move the cursor to the new page, since we moved all the records from the root.
        self.position(dest_page.page_get_infimum_rec(), dest_block.clone());
        self.page_cur.as_mut().unwrap().page_cur_search_with_match(
            dtuple,
            PAGE_CUR_L,
            &mut MatchCache::default(),
        );

        ut_ad!(self.page_cur.as_ref().unwrap().positioned_at_user_rec());

        BTree::page_set_level(&mut dest_page, level, mtr);

        // 4. We moved all the records from the root to the new page. Now, split the new record.
        self.page_split_and_insert(dtuple, n_ext, mtr);

        ut_ad!(BTree::page_get_next(&root_page) == FILL_NULL);
        ut_ad!(BTree::page_get_prev(&root_page) == FILL_NULL);
    }

    /// Attaches a newly created page to the B-tree structure after a page split operation.
    ///
    /// This method is called after a page has been split into two halves. It creates the necessary
    /// pointers in the parent node to link the new page into the B-tree structure. The method
    /// handles different split directions and updates the tree structure accordingly.
    ///
    /// # Arguments
    /// * `split_rec` - The record at which the page was split
    /// * `buf_block` - The original buffer block that was split
    /// * `new_block` - The newly created buffer block
    /// * `direction` - The direction of the split (up or down)
    /// * `mtr` - Mini-transaction for managing locks and logging
    ///
    pub fn attach_half_pages(
        &mut self,
        split_rec: Record,
        buf_block: Arc<RwLock<BufBlock>>,
        new_block: Arc<RwLock<BufBlock>>,
        direction: Direction,
        mtr: &mut Mtr,
    ) {
        // Based on the split direction, we will attach the new block.
        match direction {
            Direction::FspUp => {
                todo!("Implement FspUp direction logic");
            }
            Direction::FspDown => {
                let mut lower_page = buf_block.read().unwrap().get_page();
                let lower_page_id = buf_block.read().unwrap().get_page_id();
                let lower_page_no = lower_page_id.page_no;

                let mut upper_page = new_block.read().unwrap().get_page();
                let upper_page_id = new_block.read().unwrap().get_page_id();
                let upper_page_no = upper_page_id.page_no;

                let mut dtuple =
                    self.index.build_inner_node(split_rec, upper_page_no);

                let parent_block =
                    some_or_panic!(self.page_get_parent_block(mtr));
                let parent_page = parent_block.read().unwrap().get_page();
                let level = BTree::page_get_level(&parent_page);

                self.insert_on_non_leaf_level(level, &mut dtuple, mtr);

                // Set the next and prev pointers for the pages.
                let next_page_no = BTree::page_get_next(&lower_page);
                BTree::page_set_next(&mut upper_page, next_page_no, mtr);
                BTree::page_set_next(&mut lower_page, upper_page_no, mtr);

                BTree::page_set_prev(&mut upper_page, lower_page_no, mtr);

                if next_page_no != FILL_NULL {
                    let next_block = self.buf_pool.get(Request {
                        rw_latch: ReadWriteLatch::NoLatch,
                        page_id: PageId {
                            space_id: lower_page_id.space_id,
                            page_no: next_page_no,
                        },
                        mode: BUF_GET_ALWAYS,
                        mtr,
                    });
                    if let Some(next_block) = next_block {
                        let mut next_page =
                            next_block.read().unwrap().get_page();
                        BTree::page_set_prev(
                            &mut next_page,
                            upper_page_no,
                            mtr,
                        );
                    }
                }
            }
            _ => {
                ut_panic!("Unsupported direction for attaching half pages");
            }
        }
    }

    fn position_on_root(&mut self, mtr: &mut Mtr) {
        let root_block = self.buf_pool.get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: self.index.get_page_id(),
            mode: BUF_GET_ALWAYS,
            mtr,
        });
        let root_block = some_or_panic!(root_block);
        let root_page = root_block.read().unwrap().get_page();

        self.position(root_page.page_get_infimum_rec(), root_block);
    }

    /// Retrieves the parent block of the current page in the B-tree.
    ///
    /// This method navigates up the B-tree structure to find the parent block
    /// of the current page.
    /// It uses the current page cursor to determine
    /// the level of the current page and searches for the parent block
    /// at that level.
    ///
    /// # Arguments
    ///
    /// * `mtr` - A mutable reference to the mini-transaction used for managing locks and logging.
    ///
    /// # Returns
    ///
    /// * `Option<RwLock<BufBlock>>` - An optional reference to the parent block if found, or `None` if not.
    ///
    pub fn page_get_parent_block(
        &mut self,
        mtr: &mut Mtr,
    ) -> Option<Arc<RwLock<BufBlock>>> {
        let block = some_or_panic!(self.buf_block.clone());
        let level = BTree::page_get_level(&block.read().unwrap().get_page());

        let rec = self.page_cur.as_ref().unwrap().page_cur_get_rec();
        // ut_ad!(self.page_cur.as_ref().unwrap().positioned_at_user_rec());

        let dtuple = self.index.build_inner_node(rec, 0);

        self.position_on_root(mtr);
        self.search_to_nth_level(level + 1, &dtuple, PAGE_CUR_LE, 0, mtr);

        self.buf_block.clone()
    }

    /// Inserts a data tuple into the B-tree at a non-leaf level.
    ///
    /// This method is used during page splits and tree structure modifications
    /// to insert index entries at non-leaf levels of the B-tree. It creates a
    /// cursor, positions it at the appropriate location in the specified level,
    /// and performs a pessimistic insert operation.
    ///
    /// # Arguments
    ///
    /// * `level` - The level at which to insert (must be > 0, where 0 is leaf level)
    /// * `index` - The index structure being modified
    /// * `dtuple` - The data tuple to insert
    /// * `mtr` - Mini transaction for logging the operation
    ///
    /// # Note
    ///
    /// It is assumed that the mini-transaction (mtr) holds an exclusive latch (x-latch)
    /// on the tree during this operation.
    pub fn insert_on_non_leaf_level(
        &mut self,
        level: usize,
        dtuple: &mut DTuple,
        mtr: &mut Mtr,
    ) {
        ut_ad!(level > 0);

        self.search_to_nth_level(
            level,
            dtuple,
            PAGE_CUR_LE,
            BtrContModifyTree as usize,
            mtr,
        );
        let flag = BTR_NO_LOCKING_FLAG | BTR_KEEP_SYS_FLAG | BTR_NO_UNDO_LOG;
        let result = self.pessimistic_insert(flag, dtuple, 0, mtr);

        ut_ad!(result.is_ok());
    }
}
