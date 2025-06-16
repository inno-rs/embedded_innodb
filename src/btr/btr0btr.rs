use std::sync::{Arc, Mutex, RwLock};

use crate::btr::btr0cur::{
    BTR_KEEP_SYS_FLAG, BTR_NO_LOCKING_FLAG, BTR_NO_UNDO_LOG, BTreeCursor,
};
use crate::btr::btr0types::BtrLatchMode::BtrContModifyTree;
use crate::buf::buf0buf::BufPool;
use crate::buf::buf0types::Request;
use crate::data::data0types::DTuple;
use crate::dict::dict0types::Index;
use crate::fil::fil0types::FILL_NULL;
use crate::fsp::fsp0fsp::Fsp;
use crate::innodb::innodb0types::PageId;
use crate::lock::lock0lock::LockSys;
use crate::mtr::mtr0mtr::Mtr;
use crate::page::page0cur::PAGE_CUR_LE;
use crate::rem::rem0rec::Record;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::ut_ad;

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
                    panic!("should not happen");
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
        index: &Index,
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
            index,
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
}
