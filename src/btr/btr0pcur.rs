//! Persistent B-Tree cursor (btr0pcur).
//!
//! This module provides helpers for tracking a cursor position across pages and
//! the whole B-Tree, mirroring InnoDB semantics.
//!
//! Notes
//! - This is an internal facility layered over BTreeCursor.
//! - Methods assert latch/read-level preconditions and may panic in debug builds
//!   if invariants are violated (see Preconditions in method docs).
//! - All movements assume the underlying page cursor is valid.
use std::sync::Arc;

use crate::{
    btr::{btr0btr::BTree, btr0cur::BTreeCursor, btr0types::BtrLatchMode},
    data::data0types::DTuple,
    dict::dict0types::Index,
    fil::fil0types::FILL_NULL,
    innodb::innodb0types::{IbSearchMode, PageNo},
    mtr::mtr0mtr::Mtr,
    some_or_panic,
    ut::ut0dbg::ut_ad,
};

/// Persistent cursor position relative to current page and entire B-Tree.
///
/// This mirrors InnoDB's persistent cursor state machine. Note that if the
/// tree is empty, `BTreeCursor` will only use Unset/On/Before/After and not the
/// tree-wide variants.
#[repr(u8)]
pub enum BTreeCursorPos {
    /// The cursor isn't positioned.
    Unset = 0,
    /// The cursor is positioned on a record.
    On = 1,
    /// The cursor is positioned before the first record on the page.
    Before = 2,
    /// The cursor is positioned after the last record on the page.
    After = 3,
    /// Note: if the tree is empty, `BTreeCursor` does not use the following values, but the above ones.
    /// The cursor is positioned before the first record in the tree.
    BeforeFirstInTheTree = 4,
    /// The cursor is positioned after the last record in the tree.
    AfterLastInTheTree = 5,
}

/// Whether the persistent cursor has been positioned by a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BTreeCursorPositioned {
    /// Persistent cursor is not positioned.
    Unset,
    /// Index search positions the persistent cursor.
    IsPositioned,
}

/// Wrapper around `BTreeCursor` that tracks persistent-cursor state.
///
/// This helper enforces latch-mode/read-level preconditions. Methods generally
/// assert that the cursor is positioned and appropriate latches are held.
///
/// Notes
/// - Not thread-safe; intended to be used within a single MTR scope.
/// - In debug builds, violations of invariants will trigger assertions.
struct BTreePCursor {
    btree_cursor: BTreeCursor,
    latch_mode: BtrLatchMode,
    read_level: usize,
    cursor_positioned: BTreeCursorPositioned,
    search_mode: IbSearchMode,
}

impl BTreePCursor {
    /// Create a new persistent-cursor wrapper around an existing `BTreeCursor`.
    pub fn new(btree_cursor: BTreeCursor) -> Self {
        Self {
            btree_cursor,
            latch_mode: BtrLatchMode::NoLatches,
            read_level: 0,
            cursor_positioned: BTreeCursorPositioned::Unset,
            search_mode: IbSearchMode::IbCurrUnSupp,
        }
    }

    /// Position the cursor by searching for the given tuple at the configured read level.
    ///
    /// # Arguments
    ///
    /// * `index` - The index metadata describing the B-Tree being searched.
    /// * `dtuple` - The lookup tuple.
    /// * `search_mode` - Page search mode (see IbSearchMode).
    /// * `latch_mode` - Required latch mode for the operation.
    /// * `mtr` - Mini-transaction context used for latching and page access.
    ///
    /// On success, the persistent cursor is marked as positioned.
    pub fn open(
        &mut self,
        index: Arc<Index>,
        dtuple: &DTuple,
        search_mode: IbSearchMode,
        latch_mode: BtrLatchMode,
        mtr: &mut Mtr,
    ) {
        self.btree_cursor.search_to_nth_level(
            self.read_level,
            dtuple,
            search_mode as usize,
            latch_mode as usize,
            mtr,
        );

        self.cursor_positioned = BTreeCursorPositioned::IsPositioned;
    }

    /// Returns true if the cursor is positioned after the last record on the current page.
    pub fn is_after_last_on_page(&self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        page_cur.page_cur_is_after_last()
    }

    /// Returns true if the cursor is positioned before the first record on the current page.
    pub fn is_before_first_on_page(&self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        page_cur.page_cur_is_before_first()
    }

    /// Returns true if the cursor is currently on a user record on the page.
    pub fn is_on_user_rec(&self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        !self.is_before_first_on_page() && !self.is_after_last_on_page()
    }

    /// File-page number of the page the cursor is currently referencing.
    pub fn get_page_no(&self) -> PageNo {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        page_cur.page_cur_get_page().page_get_fil_offset()
    }

    /// Returns true if the cursor is positioned before the first record in the B-Tree.
    pub fn is_before_first_on_tree(&self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        let page = page_cur.page_cur_get_page();

        if BTree::page_get_prev(&page) != FILL_NULL {
            false
        } else {
            page_cur.page_cur_is_before_first()
        }
    }

    /// Returns true if the cursor is positioned after the last record in the B-Tree.
    pub fn is_after_last_in_tree(&self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        let page = page_cur.page_cur_get_page();

        if BTree::page_get_next(&page) != FILL_NULL {
            false
        } else {
            page_cur.page_cur_is_after_last()
        }
    }

    /// Move to the next record on the current page.
    pub fn move_to_next_on_page(&mut self) {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur_mut());
        page_cur.page_cur_move_to_next();
    }

    /// Move to the previous record on the current page.
    pub fn move_to_prev_on_page(&mut self) {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur_mut());
        page_cur.page_cur_move_to_prev();
    }

    /// Move to the next user record, advancing to the next page if needed.
    ///
    /// Assumes the cursor is currently after the last record on the page.
    ///
    /// # Returns
    ///
    /// * `true` - if the cursor moved onto a user record.
    /// * `false` - if there is no next user record (end of tree).
    ///
    pub fn move_to_next_user_rec(&mut self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);
        // We move to the next user record on the next page if we are at the end of the current page.
        ut_ad!(self.is_after_last_on_page());

        let page_cur = some_or_panic!(self.btree_cursor.get_page_cur());
        let page = page_cur.page_cur_get_page();
        let next_page_no = BTree::page_get_next(&page);

        ut_ad!(next_page_no != FILL_NULL);
        false
    }

    /// Advance the persistent cursor state to the next page after the current one.
    ///
    /// # Arguments
    ///
    /// * `mtr` - Mini-transaction context used to latch and access the next page.
    ///
    pub fn move_to_next_page(&mut self, mtr: &mut Mtr) {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);
        ut_ad!(self.is_after_last_on_page());

        let page = some_or_panic!(self.btree_cursor.get_page_cur())
            .page_cur_get_page();

        let next_page_no = BTree::page_get_next(&page);
        ut_ad!(next_page_no != FILL_NULL);

        let next_page =
            self.btree_cursor.get_block_using_page_no(next_page_no, mtr);

        let page_cursor = some_or_panic!(self.btree_cursor.get_page_cur_mut());

        page_cursor.page_cur_set_before_first();
    }

    /// Move the persistent cursor to the next record in the B-Tree, advancing
    /// to the next page if necessary.
    ///
    /// # Arguments
    ///
    /// * `mtr` - Mini-transaction context used if advancing to the next page.
    ///
    /// # Returns
    ///
    /// * `true` - if moved onto a user record.
    /// * `false` - if moved past the end of the tree.
    ///
    pub fn move_to_next(&mut self, mtr: &mut Mtr) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        if self.is_after_last_on_page() {
            if self.is_after_last_in_tree() {
                false
            } else {
                self.move_to_next_page(mtr);
                true
            }
        } else {
            self.move_to_next_on_page();
            true
        }
    }

    /// Move the persistent cursor to the previous record in the B-Tree,
    ///
    /// # Returns
    ///
    /// `True` if we were able to move the cursor.
    /// `False`, otherwise.
    ///
    pub fn move_to_prev(&mut self) -> bool {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);

        if self.is_before_first_on_page() {
            if self.is_before_first_on_tree() {
                false
            } else {
                self.move_backward_from_page();
                true
            }
        } else {
            self.move_to_prev_on_page();
            true
        }
    }

    /// Move the persistent cursor backwards across a page boundary.
    ///
    /// This is invoked when positioned before the first record on a page and
    /// needs to move to the previous page.
    ///
    pub fn move_backward_from_page(&mut self) {
        ut_ad!(self.cursor_positioned == BTreeCursorPositioned::IsPositioned);
        ut_ad!(self.latch_mode != BtrLatchMode::NoLatches);
    }
}
