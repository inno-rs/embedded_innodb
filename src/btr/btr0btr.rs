use std::{
    collections::VecDeque,
    fmt::Display,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use tracing::info;

use crate::{
    btr::btr0cur::BTreeCursor,
    buf::{
        buf0buf::BufPool,
        buf0types::{BUF_GET_ALWAYS, BufBlock, Request},
    },
    dict::dict0types::{Index, IndexId},
    fil::fil0types::{FIL_PAGE_DATA, FIL_PAGE_NEXT, FIL_PAGE_PREV, FILL_NULL},
    fsp::{fsp0fsp::Fsp, fsp0types::Direction},
    innodb::innodb0types::{PageId, PageNo, SpaceId},
    lock::lock0lock::LockSys,
    mach::mach0data::{mach_read_from_2, mach_read_from_4, mach_read_from_8},
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{
            FixMode, MemoSlot, MlogType, MlogType::MLogPageReorganize,
            MtrLogMode,
        },
    },
    page::{
        page0cur::PageCur,
        page0page::{
            PAGE_BTR_SEG_LEAF, PAGE_HEADER, PAGE_INDEX_ID, PAGE_LEVEL, Page,
            PageDebug,
        },
    },
    rem::rem0rec::Record,
    some_or_panic,
    sync::sync0rw::ReadWriteLatch,
    ut::ut0dbg::{ut_a, ut_ad},
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
    /// Fill factor for the B-tree. If a page is more than this percentage full, we will split it.
    fill_factor: usize,
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
        fill_factor: usize,
    ) -> Self {
        BTree { buf_pool, fsp, lock_sys, index, fill_factor }
    }

    pub fn get_index(&self) -> &Arc<Index> {
        &self.index
    }

    /// Returns the fill factor for the B-tree.
    ///
    /// The fill factor determines how full pages should be when
    /// operations like splits are performed. It's expressed as a percentage
    /// of the maximum page capacity.
    ///
    /// # Returns
    /// The fill factor value as a percentage (0-100)
    pub fn get_fill_factor(&self) -> usize {
        self.fill_factor
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

    /// Utility function to create a new B-tree cursor for the current btree.
    ///
    /// # Returns
    ///
    /// A new instance of `BTreeCursor` initialized with the current B-tree
    ///
    pub fn create_btr_cursor(&self) -> BTreeCursor<'_> {
        BTreeCursor::new(
            self,
            self.index.clone(),
            self.buf_pool.clone(),
            self.lock_sys.clone(),
        )
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
    ///
    /// # Arguments
    ///
    /// * `page` - The page from which to retrieve the index ID
    ///
    /// # Returns
    ///
    /// The index ID as an `IndexId` struct, which is a wrapper around a u64 value.
    ///
    pub fn page_get_index_id(page: &Page) -> IndexId {
        let page_frame = page.to_page_frame();
        let offset = PAGE_HEADER + PAGE_INDEX_ID;
        let bytes = &page_frame.0[offset..offset + 8];

        IndexId(mach_read_from_8(bytes))
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
    pub fn page_set_index_id(
        page: &mut Page,
        index_id: IndexId,
        mtr: &mut Mtr,
    ) {
        mtr.write_uint64(page, PAGE_HEADER + PAGE_INDEX_ID, index_id.0);
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
            MlogType::MLog2Bytes,
        );

        ut_ad!(Self::page_get_level(page) == level);
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
    pub fn page_alloc(
        &self,
        hint: PageNo,
        direction: Direction,
        level: usize,
        mtr: &mut Mtr,
    ) -> Option<Arc<RwLock<BufBlock>>> {
        let root_block = self.root_get(mtr);
        let root_page = root_block.read().unwrap().get_page();

        info!(
            "page_alloc: Allocating page for index {} at level {}",
            self.index.get_index_id(),
            level
        );

        let new_page_no =
            self.fsp.lock().unwrap().fseg_alloc_free_page_general(
                root_page.page_get_space_id(),
                hint,
                direction,
                true,
                mtr,
            );

        if new_page_no == FILL_NULL {
            return None;
        }

        info!(
            "page_alloc: Allocating page for index {} at level {}, page_no {}",
            self.index.get_index_id(),
            level,
            new_page_no
        );

        let buf_block = self.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId {
                space_id: root_page.page_get_space_id(),
                page_no: new_page_no,
            },
            mode: BUF_GET_ALWAYS,
            mtr,
        });

        if let Some(buf_block) = buf_block.clone() {
            let buf_block = buf_block.write().unwrap();
            let page = buf_block.get_page();

            // todo: Remove this from here.

            // Initialize the page
            let mut page =
                Page::page_create(self.index.deref(), &buf_block, Some(mtr));

            // Set the index ID for the page
            Self::page_set_index_id(&mut page, self.index.get_index_id(), mtr);

            // Set the level of the page
            Self::page_set_level(&mut page, level, mtr);
        };

        buf_block
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
        mtr.write_u32(page, FIL_PAGE_NEXT, next as u32, MlogType::MLog4Bytes);
    }

    /// Gets the next page pointer from the given page.
    ///
    /// This function retrieves the page number that the current page points to as its next page.
    ///
    /// # Arguments
    ///
    /// * `page` - The page from which to retrieve the next page pointer
    ///
    /// # Returns
    ///
    /// The page number of the next page as a `PageNo`.
    ///
    pub fn page_get_next(page: &Page) -> PageNo {
        page.get_next_page()
    }

    /// Retrieves the previous page pointer from a given page.
    ///
    /// This function returns the page number of the previous page in the B-tree structure.
    ///
    /// # Arguments
    ///
    /// * `page` - The page from which to retrieve the previous page pointer
    ///
    /// # Returns
    ///
    /// The page number of the previous page as a `PageNo`.
    ///
    pub fn page_get_prev(page: &Page) -> PageNo {
        page.get_prev_page()
    }

    /// Sets the previous page pointer in the given page.
    ///
    /// This function updates the page's previous page pointer to point to the specified page number.
    ///
    /// # Arguments
    ///
    /// * `page` - The page whose previous pointer will be updated
    /// * `prev` - The page number to set as the previous page
    /// * `mtr` - The mini-transaction that logs this operation
    pub fn page_set_prev(page: &Page, prev: PageNo, mtr: &mut Mtr) {
        mtr.write_u32(page, FIL_PAGE_PREV, prev as u32, MlogType::MLog4Bytes);
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
        &self,
        buf_block: Arc<RwLock<BufBlock>>,
        level: usize,
        mtr: &mut Mtr,
    ) {
        let buf_block = buf_block.read().unwrap();

        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix(
            buf_block.get_page_id(),
            FixMode::Exclusive
        )));

        let mut page = Page::page_create(
            self.index.deref(),
            buf_block.deref(),
            Some(mtr),
        );

        BTree::page_set_level(&mut page, level, mtr);
        BTree::page_set_index_id(&mut page, self.index.get_index_id(), mtr);

        BTree::page_set_next(&mut page, FILL_NULL, mtr);
        BTree::page_set_prev(&mut page, FILL_NULL, mtr);
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
        BTree::page_set_next(&mut page, FILL_NULL, mtr);
        BTree::page_set_prev(&mut page, FILL_NULL, mtr);
    }

    /// Retrieves the root page of a B-tree index.
    ///
    /// This function fetches the buffer block containing the root page of the B-tree index.
    /// It uses the index's page ID to locate the root page in the buffer pool and performs
    /// validation on the root file segment.
    ///
    /// # Arguments
    /// * `mtr` - The mini-transaction that will log this operation
    ///
    /// # Returns
    /// A reference-counted, thread-safe buffer block containing the root page
    pub fn root_get(&self, mtr: &mut Mtr) -> Arc<RwLock<BufBlock>> {
        let root_page_id = self.index.get_page_id();
        let mut buf_pool = self.buf_pool.lock().unwrap();
        let block = buf_pool.get(Request {
            rw_latch: ReadWriteLatch::NoLatch, /*todo: use x latch*/
            page_id: root_page_id,
            mode: BUF_GET_ALWAYS,
            mtr,
        });

        let block = some_or_panic!(block);
        let page = block.read().unwrap().get_page();
        ut_ad!(self.root_fseg_validate(&page, root_page_id.space_id));

        block
    }

    /// Validate the root file segment.
    fn root_fseg_validate(&self, page: &Page, space_id: SpaceId) -> bool {
        let fseg_header_offset = FIL_PAGE_DATA + PAGE_BTR_SEG_LEAF;

        ut_ad!(
            mach_read_from_4(&page.to_page_frame().0[fseg_header_offset..])
                == space_id as u32
        );

        true
    }

    /// Retrieves the child page number from an inner node record.
    ///
    /// This function extracts the child page number from the last field of the record,
    /// which is expected to be a 4-byte big-endian integer.
    ///
    /// # Arguments
    ///
    /// * `rec` - The record from which to extract the child page number
    ///
    /// # Returns
    ///
    /// The child page number as a `PageNo`.
    pub fn inner_node_get_child_page_no(&self, rec: &Record) -> PageNo {
        let n_fields = rec.rec_get_n_fields();
        ut_a!(rec.rec_get_n_fields() == self.index.get_n_unique_in_tree() + 1);

        let nth_field = rec.rec_get_data(n_fields - 1);
        ut_ad!(nth_field.is_some());

        let nth_field = nth_field.unwrap();
        let data = nth_field.data_or_panic();

        ut_ad!(data.len() == 4);

        // todo: Start writing child page no using mach_write_to_4
        u32::from_le_bytes(data.as_bytes().try_into().unwrap()) as PageNo
    }

    /// Reorganizes a page to create more free space.
    ///
    /// This method rearranges the records on a page to eliminate fragmentation
    /// and maximize the available free space. This is typically called when
    /// an insert operation fails due to insufficient space.
    ///
    /// # Parameters
    /// * `buf_block` - The buf block to reorganize.
    /// * `mtr` - Mini-transaction for managing locks and logging
    ///
    /// # Returns
    /// `true` if the reorganization was successful, `false` otherwise
    pub fn page_reorganize(
        &self,
        buf_block: Arc<RwLock<BufBlock>>,
        mtr: &mut Mtr,
    ) -> bool {
        let page = buf_block.read().unwrap().get_page();
        let page_id = page.page_get_page_id();

        ut_ad!(
            mtr.memo_contains(&MemoSlot::PageFix(page_id, FixMode::Exclusive))
        );

        let old_page_size = page.page_get_max_insert_size(1);

        // 1. Write the log record.
        mtr.open_and_write_index(&page, MLogPageReorganize, 0);

        // Turn logging off
        let mtr_log_mode = mtr.set_log_mode(MtrLogMode::MtrLogNone);

        // 2. Get a temp block from buf_pool.
        let temp_block = self.buf_pool.lock().unwrap().block_alloc();
        let temp_buf_block = some_or_panic!(temp_block);

        // 3. Copy the contents of the old page to the new page.
        let temp_page = temp_buf_block.write().unwrap().get_page();
        page.copy_to(temp_page.to_page_frame().0);

        // 4. Re-initialize page
        {
            let buf_block = buf_block.write().unwrap();
            Page::page_create(
                self.index.deref(),
                buf_block.deref(),
                Some(mtr),
            );
        }

        // 5. Copy the records from the temporary space to the re-created page.
        temp_page.page_copy_rec_list_end(
            buf_block.clone(),
            temp_page.page_get_infimum_rec(),
            self.index.clone(),
            mtr,
        );

        if self.index.is_clustered() == false && page.page_is_leaf() {
            let max_trx_id = temp_page.page_get_max_trx_id();
            page.page_set_max_trx_id(max_trx_id, Some(mtr));
        }

        self.lock_sys
            .lock()
            .unwrap()
            .move_reorganize_page(temp_buf_block.clone(), buf_block.clone());

        let mut success = false;
        let new_page_size = page.page_get_max_insert_size(1);
        if new_page_size != old_page_size {
            success = true;
        }

        // Free the block.
        self.buf_pool.lock().unwrap().block_free(temp_buf_block);

        // Restore logging mode
        let old_log_mode = mtr.set_log_mode(mtr_log_mode);
        ut_ad!(old_log_mode == MtrLogMode::MtrLogNone);

        success
    }

    /// Validates the entire B-tree index.
    ///
    /// This function checks the integrity of the B-tree structure by traversing
    /// from the root to the leaf nodes, ensuring that all levels of the tree
    /// are correctly structured and that the records at each level are sorted in ascending order.
    ///
    /// # Returns
    ///
    /// `true` if the B-tree is valid, `false` otherwise.
    ///
    pub fn validate_index(&self) -> bool {
        let mut mtr = Mtr::new();
        mtr.start();

        let root = self.root_get(&mut mtr);
        let root_page = root.read().unwrap().get_page();
        let root_level = Self::page_get_level(&root_page);

        for level in (0..root_level).rev() {
            if !self.level_validate(level, &mut mtr) {
                info!("BTree validation failed at level {}", level);
                return false;
            }
        }

        true
    }

    /// Validates the B-tree at a specific level.
    ///
    /// This function checks if the records at the specified level of the B-tree
    /// are sorted in ascending order.
    /// It traverses the B-tree from the root
    /// to the specified level, collecting records and checking their order.
    ///
    /// # Arguments
    ///
    /// * `level` - The level in the B-tree to validate
    /// * `mtr` - The mini-transaction used for managing locks and logging
    ///
    /// # Returns
    ///
    /// `true` if the records at the specified level are sorted,
    /// `false` otherwise.
    ///
    pub fn level_validate(&self, level: usize, mtr: &mut Mtr) -> bool {
        let root_block = self.root_get(mtr);
        let root_page = root_block.read().unwrap().get_page();
        let mut page_cursor = PageCur::new(None, root_block);
        page_cursor.page_cur_set_before_first();
        page_cursor.page_cur_move_to_next();

        let mut page = page_cursor.page_cur_get_page();

        while level != BTree::page_get_level(&page) {
            info!("{}", page_cursor.page_cur_get_rec());
            ut_ad!(page_cursor.positioned_at_user_rec());

            let current_rec = page_cursor.page_cur_get_rec();

            let child_page_no =
                self.inner_node_get_child_page_no(&current_rec);

            let mut buf_pool = self.buf_pool.lock().unwrap();
            let buf_block = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId {
                    space_id: root_page.page_get_space_id(),
                    page_no: child_page_no,
                },
                mode: BUF_GET_ALWAYS,
                mtr,
            });

            let buf_block = some_or_panic!(buf_block);

            page_cursor = PageCur::new(None, buf_block.clone());

            page = page_cursor.page_cur_get_page();
            page_cursor.page_cur_set_before_first();
            page_cursor.page_cur_move_to_next();
        }

        let records_data: Vec<String> = page_cursor
            .page_cur_get_page()
            .page_get_user_records()
            .iter()
            .map(|rec| rec.rec_get_data(0).unwrap().data_or_panic())
            .collect();

        records_data.is_sorted()
    }

    /// Formats and prints information about a non-leaf page in a B-tree.
    ///
    /// This function writes detailed information about a non-leaf (internal) page to the provided formatter.
    /// It includes page metadata (ID, level, next/previous page pointers) and information about each record
    /// in the page, including the child page numbers that the records point to.
    ///
    /// # Arguments
    ///
    /// * `buf_block` - The buffer block containing the non-leaf page to print
    /// * `f` - The formatter to write the output to
    ///
    /// # Returns
    ///
    /// A Result containing a vector of child page numbers if successful, or a formatting error
    fn print_non_leaf_block(
        &self,
        buf_block: Arc<RwLock<BufBlock>>,
        f: &mut std::fmt::Formatter<'_>,
    ) -> Result<Vec<PageNo>, std::fmt::Error> {
        write!(f, "{:-^80}\n", "Start Non-Leaf Page")?;

        let mut child_page_nos = Vec::new();
        let buf_block = buf_block.read().unwrap();
        let page = buf_block.get_page();
        write!(f, "BTree Page: {}\n", buf_block.get_page_id())?;
        write!(f, "Level: {}\n", Self::page_get_level(&page))?;
        write!(f, "Next Page: {}\n", page.get_next_page() as u64)?;
        write!(f, "Previous Page: {}\n", page.get_prev_page() as u64)?;

        write!(f, "{:-^80}\n", "Start Elements In Page")?;
        for rec in page.page_get_user_records() {
            write!(f, "Record: \n")?;
            write!(
                f,
                "Key: {}\n",
                rec.rec_get_data(0).unwrap().data_or_panic()
            )?;
            let child_page_no = self.inner_node_get_child_page_no(&rec);
            write!(f, "Child Page No(Value): {}\n", child_page_no)?;

            child_page_nos.push(child_page_no as u32);
        }

        write!(f, "{:-^80}\n", "End Elements In The Page")?;

        write!(f, "{:-^80}\n", "End Non-Leaf Page")?;

        Ok(child_page_nos)
    }

    /// Formats and prints information about a leaf page in a B-tree.
    ///
    /// This function writes detailed information about a leaf page to the provided formatter.
    /// It includes page metadata (ID, level, next/previous page pointers) and information about
    /// each record in the page. Unlike non-leaf pages, leaf pages contain the actual data records.
    ///
    /// # Arguments
    ///
    /// * `buf_block` - The buffer block containing the leaf page to print
    /// * `f` - The formatter to write the output to
    ///
    /// # Returns
    ///
    /// A Result indicating success or a formatting error
    fn print_leaf_block(
        &self,
        buf_block: Arc<RwLock<BufBlock>>,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "{:-^80}\n", "Start Leaf Page")?;
        let buf_block = buf_block.read().unwrap();
        let page = buf_block.get_page();
        write!(f, "BTree Page: {}\n", buf_block.get_page_id())?;
        write!(f, "Level: {}\n", Self::page_get_level(&page))?;
        write!(f, "Next Page: {}\n", page.get_next_page() as u64)?;
        write!(f, "Previous Page: {}\n", page.get_prev_page() as u64)?;

        write!(f, "{:-^80}\n", "Start Elements In The Page")?;

        for rec in page.page_get_user_records() {
            write!(f, "Record >> {}\n", rec)?;
        }

        write!(f, "{:-^80}\n", "End Elements In The Page")?;

        write!(f, "{:-^80}\n", "End Leaf Page")
    }

    /// Formats and prints the entire B-tree index structure.
    ///
    /// This function traverses and prints the complete B-tree index structure, including
    /// metadata about the index itself and all pages in the tree. It performs a breadth-first
    /// traversal of the tree, printing information about each page (both leaf and non-leaf).
    ///
    /// # Arguments
    ///
    /// * `f` - The formatter to write the output to
    ///
    /// # Returns
    ///
    /// A Result indicating success or a formatting error
    pub fn print_index(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "\n{:-^80}\n", "Start Index")?;

        write!(f, "Index ID: {}\n", self.index.get_index_id())?;
        write!(
            f,
            "Number of Unique Fields in Tree: {}\n",
            self.index.get_n_unique_in_tree()
        )?;
        write!(f, "Fill Factor: {}\n", self.fill_factor)?;

        let mut mtr = Mtr::new();
        mtr.start();

        let root_block = self.root_get(&mut mtr);
        let root_page = root_block.read().unwrap().get_page();
        let space_id = root_page.page_get_space_id();

        let mut pages: VecDeque<PageNo> =
            VecDeque::from(vec![self.index.get_root_page_no()]);
        let mut next_level_pages: Vec<PageNo> = vec![];

        while pages.is_empty() == false {
            let page_no = pages.pop_front().unwrap();
            let mut buf_pool = self.buf_pool.lock().unwrap();
            let buf_block = buf_pool.get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id, page_no },
                mode: BUF_GET_ALWAYS,
                mtr: &mut mtr,
            });

            let buf_block = some_or_panic!(buf_block);

            let page = buf_block.read().unwrap().get_page();

            if page.page_is_leaf() {
                self.print_leaf_block(buf_block, f)?;
            } else {
                let child_page_nos =
                    self.print_non_leaf_block(buf_block.clone(), f)?;
                next_level_pages.extend(child_page_nos);
            }

            if pages.is_empty() && next_level_pages.is_empty() == false {
                pages.extend(next_level_pages.clone());
                next_level_pages.clear();
            }
        }

        write!(f, "{:-^80}\n", "End Index")
    }
}

/// Implements the Display trait for BTree.
///
/// This implementation allows a BTree to be formatted as a string using the `{}` format specifier.
/// It provides a human-readable representation of the entire B-tree structure by delegating to
/// the `print_index` method.
impl Display for BTree {
    /// Formats the BTree for display.
    ///
    /// # Arguments
    ///
    /// * `f` - The formatter to write the output to
    ///
    /// # Returns
    ///
    /// A Result indicating success or a formatting error
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.print_index(f)
    }
}
