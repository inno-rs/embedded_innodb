use std::{
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    btr::{
        btr0cur::{
            BTR_KEEP_SYS_FLAG, BTR_NO_LOCKING_FLAG, BTR_NO_UNDO_LOG,
            BTreeCursor,
        },
        btr0types::BtrLatchMode::BtrContModifyTree,
    },
    buf::{
        buf0buf::BufPool,
        buf0types::{BufBlock, Request},
    },
    data::data0types::DTuple,
    dict::dict0types::Index,
    fil::fil0types::{FIL_PAGE_NEXT, FILL_NULL},
    fsp::fsp0fsp::Fsp,
    innodb::innodb0types::{PageId, PageNo},
    lock::lock0lock::LockSys,
    mach::mach0data::{mach_read_from_2, mach_read_from_8},
    mtr::{
        mtr0mtr::{LatchHandle, LockMode, MemoSlot, Mtr},
        mtr0types::{FixMode, MlogType},
    },
    page::{
        page0cur::PAGE_CUR_LE,
        page0page::{PAGE_HEADER, PAGE_INDEX_ID, PAGE_LEVEL, Page},
    },
    rem::rem0rec::Record,
    sync::sync0rw::ReadWriteLatch,
    ut::ut0dbg::ut_ad,
    ut_panic,
};

/// BTree represents a B-tree data structure used for efficient data storage and retrieval.
///
/// A B-tree is a self-balancing tree data structure that maintains sorted data and allows
/// searches, sequential access, insertions, and deletions in logarithmic time.
pub struct BTree {
    /// BufferPool manager for handling page caching and I/O operations.
    buf_pool: Arc<Mutex<BufPool>>,
    /// File segment manager for handling file space allocation.
    fsp: Arc<Mutex<Fsp>>,
    /// Lock Management system.
    lock_sys: Arc<Mutex<LockSys>>,
    /// Reference to index.
    index: Arc<Index>,
}

impl BTree {
    /// Creates a new BTree instance.
    ///
    /// # Parameters
    /// * `buf_pool` - A reference to the buffer pool manager for page caching
    /// * `fsp` - A reference to the file space manager for space allocation
    ///
    /// # Returns
    /// A new BTree instance initialized with the provided buffer pool and file space manager
    pub fn new(
        buf_pool: Arc<Mutex<BufPool>>,
        fsp: Arc<Mutex<Fsp>>,
        lock_sys: Arc<Mutex<LockSys>>,
        index: Arc<Index>,
    ) -> Self {
        BTree { buf_pool, fsp, lock_sys, index }
    }

    pub fn get_index(&self) -> &Arc<Index> {
        &self.index
    }

    /// Retrieves the previous user record in the B-tree.
    ///
    /// This method navigates to the previous record in the B-tree, potentially
    /// crossing page boundaries if necessary.
    ///
    /// # Parameters
    /// * `rec` - The current record from which to find the previous record
    /// * `mtr` - An optional mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// * `Some(Record)` - The previous user record if it exists
    /// * `None` - If there is no previous record
    pub fn get_prev_user_rec(
        &self,
        rec: Record,
        mtr: Option<Mtr>,
    ) -> Option<Record> {
        let page = rec.get_page();
        if !page.page_rec_is_infimum(rec) {
            let prev_rec = rec.rec_get_prev();
            ut_ad!(prev_rec.is_some());

            match prev_rec {
                None => {}
                Some(prev) => {
                    if page.page_rec_is_infimum(prev) == false {
                        return Some(prev);
                    }
                }
            }
        }

        let prev_page_no = page.get_prev_page();
        let mut mtr = mtr.unwrap();
        if prev_page_no != FILL_NULL {
            // Fetch the page from buffer_pool
            let mut pool = self.buf_pool.lock().unwrap();
            let buf_block = pool.get(Request {
                rw_latch: ReadWriteLatch::SLatch,
                page_id: PageId {
                    space_id: page.page_get_space_id(),
                    page_no: prev_page_no,
                },
                mode: 0,
                mtr: &mut mtr,
            });

            match buf_block {
                None => {
                    ut_panic!("should not happen")
                }
                Some(buf) => {
                    let prev_page = buf.read().unwrap().get_page();
                    let sup = prev_page.page_get_supremum_rec();
                    return sup.rec_get_prev();
                }
            }
        }

        None
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
        index: Arc<Index>,
        dtuple: &mut DTuple,
        mtr: &mut Mtr,
    ) {
        ut_ad!(level > 0);

        let mut btr_cursor = BTreeCursor::new(
            self,
            self.index.clone(),
            self.buf_pool.clone(),
            self.lock_sys.clone(),
        );

        btr_cursor.search_to_nth_level(
            vec![],
            level,
            dtuple,
            PAGE_CUR_LE,
            BtrContModifyTree as usize,
            mtr,
        );

        let flag = BTR_NO_LOCKING_FLAG | BTR_KEEP_SYS_FLAG | BTR_NO_UNDO_LOG;
        let result = btr_cursor.pessimistic_insert(flag, dtuple, 0, mtr);

        ut_ad!(result.is_ok());
    }

    /// Retrieves the next user record in the B-tree.
    ///
    /// This method navigates to the next record in the B-tree, potentially
    /// crossing page boundaries if necessary.
    ///
    /// # Parameters
    /// * `rec` - The current record from which to find the next record
    /// * `mtr` - An optional mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// * `Some(Record)` - The next user record if it exists
    /// * `None` - If there is no next record
    pub fn get_next_user_rec(
        &self,
        rec: Record,
        mtr: Option<Mtr>,
    ) -> Option<Record> {
        let page = rec.get_page();
        if !page.page_rec_is_supremum(rec) {
            let next_rec = rec.rec_get_next();
            ut_ad!(next_rec.is_some());

            match next_rec {
                None => {}
                Some(prev) => {
                    if page.page_rec_is_supremum(prev) == false {
                        return Some(prev);
                    }
                }
            }
        }

        let next_page_no = page.get_next_page();
        let mut mtr = mtr.unwrap();
        if next_page_no != FILL_NULL {
            // Fetch the page from buffer_pool
            let mut pool = self.buf_pool.lock().unwrap();
            let buf_block = pool.get(Request {
                rw_latch: ReadWriteLatch::SLatch,
                page_id: PageId {
                    space_id: page.page_get_space_id(),
                    page_no: next_page_no,
                },
                mode: 0,
                mtr: &mut mtr,
            });

            match buf_block {
                None => {
                    panic!("should not happen");
                }
                Some(buf) => {
                    let next_page = buf.read().unwrap().get_page();
                    let sup = next_page.page_get_supremum_rec();
                    return sup.rec_get_next();
                }
            }
        }

        None
    }

    /// Get the index id from the page.
    pub fn page_get_index_id(page: &Page) -> usize {
        let page_frame = page.to_page_frame();
        let offset = PAGE_HEADER + PAGE_INDEX_ID;
        let bytes = &page_frame.0[offset..offset + 8];

        mach_read_from_8(bytes) as usize
    }

    /// Sets the index ID for a page.
    ///
    /// This method updates the index ID field in the page header. The index ID
    /// is used to identify which index the page belongs to. This operation should
    /// be logged in the mini-transaction (mtr) for recovery purposes.
    ///
    /// # Arguments
    ///
    /// * `page` - The page whose index ID will be set
    /// * `index_id` - The index_id of the page in the B-tree
    /// * `mtr` - The mini-transaction that logs this operation
    ///
    /// # Note
    ///
    /// This method is currently not implemented (marked as todo).
    pub fn page_set_index_id(page: &mut Page, index_id: usize, mtr: &mut Mtr) {
        mtr.write_uint64(page, PAGE_HEADER + PAGE_INDEX_ID, index_id as u64);
    }

    /// Get the node level field in an index page.
    pub fn page_get_level(page: &Page) -> usize {
        let page_frame = page.to_page_frame();
        let offset = PAGE_HEADER + PAGE_LEVEL;
        let bytes = &page_frame.0[offset..offset + 2];

        mach_read_from_2(bytes) as usize
    }

    /// Sets the level of a page in the B-tree.
    ///
    /// This method updates the level field in the page header. The level indicates
    /// the page's position in the B-tree hierarchy (0 for leaf pages, higher values
    /// for non-leaf pages). This operation should be logged in the mini-transaction (mtr)
    /// for recovery purposes.
    ///
    /// # Arguments
    ///
    /// * `page` - The page whose level will be set
    /// * `level` - The level value to set (0 for leaf pages)
    /// * `mtr` - The mini-transaction that logs this operation
    ///
    /// # Note
    ///
    /// This method is currently commented out and not fully implemented.
    pub fn page_set_level(page: &mut Page, level: usize, mtr: &mut Mtr) {
        // todo: Add here what is the size of the tree at level 16
        ut_ad!(level < 16);

        mtr.write_u32(
            &page,
            PAGE_HEADER + PAGE_LEVEL,
            level as u32,
            MlogType::MLOG_2BYTES,
        );
    }

    /// Gets the child page number from a node pointer record.
    ///
    /// The node pointer record contains a pointer to a child page in its last field.
    /// This function extracts and returns that page number.
    pub fn node_ptr_get_child_page_no(rec: &Record) -> usize {
        let n_fields = rec.rec_get_n_fields();
        // The last field is a system field, and it houses the pointer to the next page.
        let nth_field = rec.rec_get_data(n_fields - 1);

        usize::from_le_bytes(
            nth_field.unwrap().data_or_panic().as_bytes().try_into().unwrap(),
        )
    }

    /// Allocates a new page for the B-tree at the specified level.
    ///
    /// This function allocates a new page that will be used in the B-tree structure.
    /// The level parameter specifies at which level in the tree the page will be used.
    /// The operation is logged in the mini-transaction (mtr).
    ///
    /// # Arguments
    /// * `level` - The level in the B-tree where the page will be used
    /// * `mtr` - The mini-transaction that logs this operation
    ///
    /// # Returns
    /// A reference to the newly allocated buffer block
    pub fn page_alloc(level: usize, mtr: &mut Mtr) -> Arc<RwLock<BufBlock>> {
        todo!("implement")
    }

    /// Sets the next page pointer in the given page.
    ///
    /// This function updates the page's next page pointer to point to the specified page number.
    /// The operation is logged in the mini-transaction (mtr).
    ///
    /// # Arguments
    /// * `page` - The page whose next pointer will be updated
    /// * `next` - The page number to set as the next page
    /// * `mtr` - The mini-transaction that logs this operation
    pub fn page_set_next(page: &Page, next: PageNo, mtr: &mut Mtr) {
        mtr.write_u32(page, FIL_PAGE_NEXT, next as u32, MlogType::MLOG_4BYTES);
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
        &self,
        page_no: PageNo,
        root: &Page,
        dtuple: &DTuple,
        n_ext: usize,
        mtr: &mut Mtr,
    ) {
        let space_id = root.page_get_space_id();
        let page_no = page_no;

        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix(
            PageId { space_id, page_no },
            FixMode::Exclusive,
        )));

        ut_ad!(mtr.memo_contains(&MemoSlot::Latch(
            LatchHandle(),
            LockMode::Exclusive,
        )));

        let level = Self::page_get_level(root);

        let new_block = Self::page_alloc(level, mtr);
    }

    /// Creates and initializes a B-tree page.
    ///
    /// This function creates a new page for use in a B-tree structure. It initializes
    /// the page with the appropriate index ID and level information. The operation is
    /// logged in the mini-transaction (mtr).
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block where the page will be created
    /// * `index` - The index to which this page belongs
    /// * `level` - The level in the B-tree where the page will be used
    /// * `mtr` - The mini-transaction that logs this operation
    pub fn page_create(
        buf_block: Arc<RwLock<BufBlock>>,
        index: Arc<Index>,
        level: usize,
        mtr: &mut Mtr,
    ) {
        let buf_block = buf_block.read().unwrap();

        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix(
            buf_block.get_page_id(),
            FixMode::Exclusive
        )));

        let mut page =
            Page::page_create(index.deref(), buf_block.deref(), Some(mtr));

        BTree::page_set_level(&mut page, level, mtr);
        BTree::page_set_index_id(&mut page, index.get_index_id(), mtr);
    }

    /// Creates an empty page in a B-tree at the specified level.
    ///
    /// This function initializes a new empty page in the B-tree structure. It creates
    /// a page using `Page::page_create` and sets its level in the B-tree hierarchy.
    /// The operation is logged in the mini-transaction (mtr).
    ///
    /// # Arguments
    /// * `buf_block` - The buffer block where the page will be created
    /// * `level` - The level in the B-tree where the page will be used
    /// * `mtr` - The mini-transaction that logs this operation
    pub fn page_empty(
        &self,
        buf_block: Arc<RwLock<BufBlock>>,
        level: usize,
        mtr: &mut Mtr,
    ) {
        let page_id = buf_block.read().unwrap().get_page_id();

        ut_ad!(
            mtr.memo_contains(&MemoSlot::PageFix(page_id, FixMode::Exclusive))
        );

        let mut page = {
            let buf_block = buf_block.write().unwrap();
            Page::page_create(self.index.deref(), buf_block.deref(), Some(mtr))
        };

        Self::page_set_level(&mut page, level, mtr);
    }
}
