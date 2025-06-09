use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use crate::buf::buf0types::BufBlock;
use crate::data::data0types::DTuple;
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::IbSearchMode::{
    IbCurG, IbCurGe, IbCurL, IbCurLe, IbCurrUnSupp,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::page::page0page::{
    PAGE_DIR_SLOT_MAX_N_OWNED, PAGE_DIR_SLOT_MIN_N_OWNED, PAGE_N_RECS, Page,
};
use crate::rem::rem0rec::Record;
use crate::ut::ut0dbg::ut_ad;

/// Search modes for positioning a page cursor.
///
/// These constants define how the cursor should be positioned when the exact search key
/// is not found on a page. They are used in various search operations to determine
/// the cursor position relative to the search key.

/// Unsupported search mode.
pub const PAGE_CUR_UNSUPP: usize = IbCurrUnSupp as usize;

/// Position the cursor on the record greater than the search key.
/// If the search key is not found, the cursor will be placed on the first record
/// that is greater than the search key.
pub const PAGE_CUR_G: usize = IbCurG as usize;

/// Position the cursor on the record greater than or equal to the search key.
/// If the search key is found, the cursor will be placed on that record.
/// Otherwise, it will be placed on the first record greater than the search key.
pub const PAGE_CUR_GE: usize = IbCurGe as usize;

/// Position the cursor on the record less than the search key.
/// If the search key is not found, the cursor will be placed on the last record
/// that is less than the search key.
pub const PAGE_CUR_L: usize = IbCurL as usize;

/// Position the cursor on the record less than or equal to the search key.
/// If the search key is found, the cursor will be placed on that record.
/// Otherwise, it will be placed on the last record less than the search key.
pub const PAGE_CUR_LE: usize = IbCurLe as usize;

/// A cursor structure for navigating through records in a page.
///
/// This cursor is used to traverse, insert, and delete records on a page.
/// It maintains a reference to the current record, the page, and the index.
pub struct PageCur {
    /// The index to which the cursor belongs
    index: Arc<Index>,
    /// The record to which the cursor points
    rec: Record,
    /// The buffer block containing the page
    block: BufBlock,
    /// The page on which the cursor operates
    page: Page,
}

impl PageCur {
    /// Creates a new page cursor.
    ///
    /// # Arguments
    ///
    /// * `index`: The index to which the cursor belongs.
    /// * `rec`: The record to which the cursor points.
    /// * `block`: The buffer block containing the page.
    ///
    /// # Returns
    ///
    /// * A new `PageCur` instance.
    pub fn new(
        index: Arc<Index>,
        rec: Record,
        block: BufBlock,
        page: Page,
    ) -> Self {
        PageCur { index, rec, block, page }
    }

    /// Gets the record to which the cursor points.
    ///
    /// # Returns
    ///
    /// * The record to which the cursor points.
    pub fn page_cur_get_rec(&self) -> Record {
        self.rec.clone()
    }

    /// Gets the buffer block associated with the cursor.
    ///
    /// # Returns
    ///
    /// * The buffer block associated with the cursor.
    fn page_cur_get_block(&self) -> BufBlock {
        self.block.clone()
    }

    /// Sets the cursor object to point before the first user record on the page.
    ///
    /// # Arguments
    ///
    /// * `&mut self`: Mutable reference to the cursor.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn page_cur_set_before_first(&mut self) {
        self.rec = self.page.page_get_infimum_rec();
    }

    /// Sets the cursor object to point after the last user record on the page.
    ///
    /// # Arguments
    ///
    /// * `&mut self`: Mutable reference to the cursor.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn page_cur_set_after_last(&mut self) {
        self.rec = self.page.page_get_supremum_rec();
    }

    /// Returns true if the cursor is before the first user record on the page.
    ///
    /// # Returns
    ///
    /// * `true` if the cursor is before the first user record, `false` otherwise.
    ///
    pub fn page_cur_is_before_first(&self) -> bool {
        self.page.page_rec_is_infimum(self.rec)
    }

    /// Returns true if the cursor is after the last user record on the page.
    ///
    /// # Returns
    ///
    /// * `true` if the cursor is after the last user record, `false` otherwise.
    pub fn page_cur_is_after_last(&self) -> bool {
        self.page.page_rec_is_supremum(self.rec)
    }

    /// Moves the cursor to the next record on the page.
    ///
    /// # Arguments
    ///
    /// * `&mut self`: Mutable reference to the cursor.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    ///
    /// # Panics
    ///
    /// * If the cursor is already after the last record.
    /// * If no next record is found.
    fn page_cur_move_to_next(&mut self) {
        if self.page_cur_is_after_last() {
            panic!("Cursor is after the last record");
        }
        match self.rec.rec_get_next() {
            Some(next_rec) => self.rec = next_rec,
            None => panic!("No next record found"),
        }
    }

    /// Moves the cursor to the previous record on the page.
    ///
    /// # Arguments
    ///
    /// * `&mut self`: Mutable reference to the cursor.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    ///
    /// # Note
    ///
    /// * This method is not yet implemented.
    fn page_cur_move_to_prev(&mut self) {
        todo!("Implement move to previous record");
    }

    /// Deletes a record at the page cursor. The cursor is moved to the next record after the deleted one.
    ///
    /// # Arguments
    ///
    /// * `&mut self`: Mutable reference to the cursor.
    /// * `index`: The index to which the record belongs.
    /// * `mtr`: Optional Mini-Transaction object for logging.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    ///
    /// # Note
    ///
    /// * This function handles all the necessary steps for record deletion including
    ///   - Updating the linked list of records
    ///   - Updating directory slots if needed
    ///   - Freeing memory occupied by the record
    ///   - Balancing slots if necessary
    pub fn page_cur_delete_rec(&mut self, index: Index, mtr: Option<Mtr>) {
        let current_rec = self.rec;

        ut_ad!(self.page.page_rec_is_user_rec(current_rec));

        let cur_slot_no = self.page.page_dir_find_owner_slot(current_rec);
        let cur_n_owned = current_rec.rec_get_n_owned();

        /* 0. Write the log record. */
        // page_cur_delete_rec_write_log

        /* 1. Reset the last insert info in the page header
        and increment the modify-clock of the frame. */

        // buf_block_modify_clock_inc

        ut_ad!(cur_slot_no > 0);
        let mut rec = self.page.page_dir_slot_get_rec(cur_slot_no - 1);

        /* The rec now points to the record of the previous directory slot.
        Look for the immediate predecessor of the current_rec in a loop.
         */

        let mut prev_rec: Option<Record> = Option::None;
        while rec != current_rec {
            prev_rec = Some(rec);
            rec = rec.rec_get_next().unwrap()
        }

        self.page_cur_move_to_next();

        let next_rec = self.rec;

        /* 3. Remove the record from the linked list of records. */
        self.page.page_rec_set_next(prev_rec.unwrap(), Some(next_rec));

        /* 4. If the deleted record is pointed to by a dir slot, then we update the record pointer in the slot. */

        if current_rec == self.page.page_dir_slot_get_rec(cur_slot_no) {
            self.page.page_dir_slot_set_rec(cur_slot_no, prev_rec.unwrap());
        }

        /* 5. Update the number of the owned records of the slot. */
        self.page.page_dir_set_n_owned(cur_slot_no, cur_n_owned - 1);

        /* 6. Free the memory occupied by the record. */
        self.page.page_mem_free(current_rec);

        /* 7. Now we have decremented the number of the owned records of the slot.
        If the number drops below PAGE_DIR_SLOT_MIN_N_OWNED, we balance the slots.*/

        if cur_n_owned <= PAGE_DIR_SLOT_MIN_N_OWNED {
            // page_dir_balance_slot
        }
    }

    pub fn page_cur_tuple_insert(
        &self,
        dtuple: &DTuple,
        index: &Index,
        n_ext: usize,
        mtr: &mut Mtr,
    ) -> Option<Record> {
        let mut record = Record::new(null_mut());
        record = record.rec_convert_dtuple_to_rec(index, dtuple, n_ext);

        self.page_cur_insert_rec_low(record, mtr)
    }

    /// Inserts a record next to the page cursor on an uncompressed page.
    /// The cursor stays at the same place.
    ///
    /// # Arguments
    ///
    /// * `&self`: Reference to the cursor.
    /// * `rec`: The record to be inserted.
    /// * `mtr`: Mini-Transaction object.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    ///
    /// # Note
    ///
    /// * This function handles the following steps for record insertion:
    ///   - Verifies the cursor is not at the supremum record
    ///   - Copies the record to be inserted
    ///   - Inserts the record in the linked list of records
    ///   - Updates the page header to increment the record count
    ///   - Sets the n_owned field in the inserted record to zero
    pub fn page_cur_insert_rec_low(
        &self,
        rec: Record,
        mtr: &mut Mtr,
    ) -> Option<Record> {
        ut_ad!(!self.page.page_rec_is_supremum(self.rec));

        /* 1. Get the size of the physical record. */
        let rec_size = rec.rec_offs_size();

        /* 2. Find a suitable space from page memory management. */
        let mut insert_rec_opt: Option<Record> = None;

        let free_list = self.page.page_header_get_page_free();
        match free_list {
            Some(head) => {
                todo!("found some element in the free list, utilize it.")
            }
            None => {
                insert_rec_opt = self.page.page_mem_alloc_heap(rec_size);
            }
        }

        let mut insert_rec = insert_rec_opt.unwrap();

        /* 3. Copy the record. */
        rec.rec_copy(&mut insert_rec);

        /* 4. Insert the record in the linked list of records. */
        let next_rec = self.rec.rec_get_next().unwrap();
        self.page.page_set_next_rec(insert_rec, Some(next_rec));
        self.page.page_set_next_rec(self.rec, Some(insert_rec));

        self.page.page_header_set_field(
            PAGE_N_RECS,
            1 + self.page.page_header_get_field(PAGE_N_RECS),
        );

        /* 5. Set the n_owned field in the inserted record to zero and set the heap no field.*/

        insert_rec.rec_set_n_owned(0);

        /* 7. It remains to update the owner record. */
        let owner_rec = self.page.page_rec_find_owner_rec(insert_rec);
        let n_owned = owner_rec.rec_get_n_owned();

        owner_rec.rec_set_n_owned(n_owned + 1);

        /* 8. Now we have incremented the n_owner fields of owner record.
        If the number exceeds PAGE_DIR_SLOT_MAX_N_OWNED,
        we have to split the corresponding directory slot. */

        if n_owned == PAGE_DIR_SLOT_MAX_N_OWNED {
            self.page.page_dir_split_slot(
                self.page.page_dir_find_owner_slot(owner_rec),
            );
        }

        /* 9. Write the log record of the insert. */

        insert_rec_opt
    }
}
