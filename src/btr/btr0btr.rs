use std::sync::{Arc, Mutex, RwLock};

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
    fil::fil0types::FILL_NULL,
    fsp::fsp0fsp::Fsp,
    innodb::innodb0types::{PageId, PageNo},
    lock::lock0lock::LockSys,
    mtr::{
        mtr0mtr::{LatchHandle, LockMode, MemoSlot, Mtr},
        mtr0types::FixMode,
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
    buf_pool: Arc<RwLock<BufPool>>,
    /// File segment manager for handling file space allocation.
    fsp: Arc<Mutex<Fsp>>,
    /// Lock Management system.
    lock_sys: Arc<Mutex<LockSys>>,
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
        buf_pool: Arc<RwLock<BufPool>>,
        fsp: Arc<Mutex<Fsp>>,
        lock_sys: Arc<Mutex<LockSys>>,
    ) -> Self {
        BTree { buf_pool, fsp, lock_sys }
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
            let mut pool = self.buf_pool.write().unwrap();
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
        index: Arc<Mutex<Index>>,
        dtuple: &mut DTuple,
        mtr: &mut Mtr,
    ) {
        ut_ad!(level > 0);

        let mut btr_cursor = BTreeCursor::new(
            index,
            self.fsp.clone(),
            self.lock_sys.clone(),
            self,
            self.buf_pool.clone(),
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
            let mut pool = self.buf_pool.write().unwrap();
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

        usize::from_le_bytes(bytes.try_into().unwrap())
    }

    /// Get the node level field in an index page.
    pub fn page_get_level(page: &Page) -> usize {
        let page_frame = page.to_page_frame();
        let offset = PAGE_HEADER + PAGE_LEVEL;
        let bytes = &page_frame.0[offset..offset + 2];

        u16::from_le_bytes(bytes.try_into().unwrap()) as usize
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
        // mtr.mlog().mlog_write_u32(
        //     page.to_page_frame().0,
        //     FIL_PAGE_NEXT,
        //     next as usize,
        //     MlogType::MLOG_4BYTES,
        //     mtr,
        // );
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

        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix {
            page_id: PageId { space_id, page_no },
            mode: FixMode::Exclusive,
        }));

        ut_ad!(mtr.memo_contains(&MemoSlot::Latch {
            handle: LatchHandle(),
            mode: LockMode::Exclusive,
        }));

        let level = Self::page_get_level(root);

        let new_block = Self::page_alloc(level, mtr);
    }
}
