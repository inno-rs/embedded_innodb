use std::{
    cmp::Ordering,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    btr::{btr0btr::BTree, btr0types::BTR_ESTIMATE},
    buf::{
        buf0buf::BufPool,
        buf0types::{BUF_GET_ALWAYS, BufBlock, Request},
    },
    data::data0types::DTuple,
    dict::dict0types::Index,
    innodb::innodb0types::{
        DbErr,
        DbErr::{DbFail, DbSuccess, DbTooBigRecord},
        PageId,
    },
    lock::lock0lock::LockSys,
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{FixMode, MemoSlot},
    },
    page::{
        page0cur::{MatchCache, PageCur},
        page0page::Page,
    },
    rem::{rem0cmp::cmp_dtuple_rec, rem0rec::Record},
    some_or_panic,
    sync::sync0rw::ReadWriteLatch,
    ut::ut0dbg::ut_ad,
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
    buf_pool: Arc<Mutex<BufPool>>,
    /// Reference to LockSys.
    lock_sys: Arc<Mutex<LockSys>>,
}

impl<'a> BTreeCursor<'a> {
    /// Creates a new BTreeCursor instance.
    ///
    /// # Parameters
    /// * `index` - A reference to the index being accessed
    /// * `fsp` - A reference to the file space manager
    /// * `btree` - A reference to the B-tree being traversed
    ///
    /// # Returns
    /// A new BTreeCursor instance initialized with the provided references
    pub fn new(
        btree: &'a BTree,
        index: Arc<Index>,
        buf_pool: Arc<Mutex<BufPool>>,
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
        }
    }

    /// Determines if a page is a leaf page in the B-tree.
    ///
    /// Leaf pages contain actual data records, while non-leaf pages contain
    /// pointers to other pages.
    ///
    /// # Parameters
    /// * `page` - The page to check
    ///
    /// # Returns
    /// `true` if the page is a leaf page, `false` otherwise
    pub fn page_is_leaf(page: Page) -> bool {
        false
    }

    /// Tries to perform an optimistic insert to a page in an index tree.
    ///
    /// This method attempts to insert a record next to the cursor position.
    /// It is assumed
    /// that the mini-transaction (Mtr) holds an exclusive latch (x-latch) on the page.
    /// The operation will not succeed if there is not enough space on the page.
    /// If there is only one record on the page, the operation will always succeed.
    ///
    /// # Parameters
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

        ut_ad!(
            mtr.memo_contains(&MemoSlot::PageFix(page_id, FixMode::Exclusive))
        );

        let max_size = page.page_get_max_insert_size_after_reorganize(1);
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

        // If there have been many consecutive inserts on the page, and we are on the leaf, check if we have to split the page to reserve enough free space for future updates of records.

        if is_clustered
            && page.page_n_get_recs() >= 2
            && Self::page_is_leaf(page)
            && page.page_get_max_insert_size(1) < rec_size
        {
            return Err(DbFail(
                "Optimistic insert failed: too many records on the page"
                    .to_string(),
            ));
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
            let success = self.page_reorganize(self.index.deref(), mtr);
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
            Ok(_) => {}
            Err(e) => {
                if matches!(e, DbFail(_)) {
                    return Err(e);
                }
            }
        }

        // 2. We were not able to insert. Try pessimistic insert.

        Ok(())
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
    /// * `DbSuccess` - If the operation can proceed
    /// * Other `DbErr` values - If there are locking conflicts or other issues
    pub fn ins_lock_and_undo(
        &self,
        flags: usize,
        dtuple: &DTuple,
        mtr: &mut Mtr,
    ) -> Result<(), DbErr> {
        let rec = self.get_rec().unwrap();

        {
            let lock_sys = self.lock_sys.lock().unwrap();
            let buf_block = self.buf_block.as_ref().unwrap();
            let buf_block_ref = buf_block.read().unwrap();
            let err = lock_sys.rec_insert_check_and_lock(
                flags,
                rec,
                buf_block_ref.deref(),
                self.index.deref(),
                mtr,
            );

            if err != DbSuccess {
                return Err(err);
            }
        }

        Ok(())
    }

    /// Reorganizes a page to create more free space.
    ///
    /// This method rearranges the records on a page to eliminate fragmentation
    /// and maximize the available free space. This is typically called when
    /// an insert operation fails due to insufficient space.
    ///
    /// # Parameters
    /// * `index` - The index to which the page belongs
    /// * `mtr` - Mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// `true` if the reorganization was successful, `false` otherwise
    pub fn page_reorganize(&self, index: &Index, mtr: &mut Mtr) -> bool {
        // Reorganize the page.
        true
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
    pub fn search_to_nth_level(
        &mut self,
        paths: Vec<Path>,
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
        let PageId { space_id, page_no } = self.index.get_page_id();

        // Initialize a cache for matching records
        let mut match_cache: MatchCache = Default::default();

        // Search loop - continues until we find the target position
        let mut found = false;
        while found == false {
            // Get the buffer block for the current page
            let block = self.buf_pool.lock().unwrap().get(Request {
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

            // Verify that the page belongs to the correct index
            ut_ad!(
                BTree::page_get_index_id(&page) == self.index.get_index_id()
            );

            if height.is_none() {
                // We are in the root node, initialize the tree height
                let level = BTree::page_get_level(&page);
                height.replace(level);
                self.tree_height = level + 1;
            }

            let height_val = some_or_panic!(height);
            if height_val == 0 {
                // We have reached the leaf level
                if latch == ReadWriteLatch::NoLatch {
                    // Unlatch the page, and then return.
                }

                found = true;
            }

            // Create a page cursor for the current page
            let mut page_cur =
                PageCur::new(self.index.clone(), None, block.clone(), page);

            // Search for the position of the data tuple in the current page
            page_cur.page_cur_search_with_match(
                dtuple,
                mode,
                &mut match_cache,
            );

            self.position(page_cur.page_cur_get_rec(), block);
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
        self.page_cur = Some(PageCur::new(
            self.index.clone(),
            Some(rec),
            buf_block.clone(),
            page,
        ))
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
        let mut rec = page.page_get_infimum_rec().rec_get_next();

        // We linearly keep on including records and keep on checking if the
        // total accumulated size is less than total_space / 2.
        let mut dtuple_included = false;
        while incl_data + Page::page_dir_calc_reserved_space(total_n_recs)
            < total_space / 2
        {
            if let Some(record) = rec {
                if record == insert_rec && dtuple_included == false {
                    incl_data += insert_size;
                    dtuple_included = true;
                } else {
                    incl_data += record.rec_offs_size();
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
            if split_rec == insert_rec {
                let next_rec = split_rec.rec_get_next();
                return if let Some(rec) = next_rec
                    && page.page_rec_is_supremum(rec) == false
                {
                    Some(rec)
                } else {
                    next_rec
                };
            } else {
                return Some(split_rec);
            }
        }

        None
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
}
