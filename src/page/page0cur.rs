use crate::buf::buf0types::BufBlock;
use crate::dict::dict0types::Index;
use crate::page::page0page::Page;
use crate::rem::rem0rec::Record;
use std::sync::{Arc, Mutex};

struct PageCur {
    index: Arc<Mutex<Index>>,
    rec: Record,
    block: BufBlock,
    page: Page,
}

impl PageCur {
    fn new(index: Arc<Mutex<Index>>, rec: Record, block: BufBlock) -> Self {
        let frame = block.getFrame();
        PageCur { index, rec, block, page: Page::new(frame) }
    }

    fn page_cur_get_rec(&self) -> Record {
        self.rec.clone()
    }

    fn page_cur_get_block(&self) -> BufBlock {
        self.block.clone()
    }

    /// Sets the cursor object to point before the first user record on the page.
    ///
    fn page_cur_set_before_first(&mut self) {
        self.rec = self.page.page_get_infimum_rec();
    }

    /// Sets the cursor object to point after the last user record on the page.
    fn page_cur_set_after_last(&mut self) {
        self.rec = self.page.page_get_supremum_rec();
    }

    /// Returns true if the cursor is before the first user record on the page.
    ///
    /// # Returns
    ///
    /// * `true` if the cursor is before the first user record, `false` otherwise.
    ///
    fn page_cur_is_before_first(&self) -> bool {
        self.page.page_rec_is_infimum(self.rec)
    }

    /// Returns true if the cursor is after the last user record on the page.
    ///
    /// # Returns
    ///
    /// * `true` if the cursor is after the last user record, `false` otherwise.
    fn page_cur_is_after_last(&self) -> bool {
        self.page.page_rec_is_supremum(self.rec)
    }

    /// Moves the cursor to the next record on the page.
    fn page_cur_move_to_next(&mut self) {
        if self.page_cur_is_after_last() {
            panic!("Cursor is after the last record");
        }
        match self.rec.rec_get_next() {
            Some(next_rec) => self.rec = next_rec,
            None => panic!("No next record found"),
        }
    }

    fn page_cur_move_to_prev(&mut self) {
        todo!("Implement move to previous record");
    }
}
