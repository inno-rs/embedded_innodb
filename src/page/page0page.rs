use std::ptr::slice_from_raw_parts_mut;

use tracing::{info, warn};

use crate::buf::buf0types::BufBlock;
use crate::data::data0types::{
    DATA_ENGLISH, DATA_NOT_NULL, DATA_VARCHAR, DTuple,
};
use crate::dict::dict0types::Index;
use crate::fil::fil0types::{
    FIL_PAGE_DATA, FIL_PAGE_DATA_END, FIL_PAGE_NEXT, FIL_PAGE_OFFSET,
    FIL_PAGE_PREV, FIL_PAGE_SPACE_ID, FSEG_PAGE_DATA,
};
use crate::innodb::innodb0types::{
    PageFrame, PageNo, SpaceId, UNIV_PAGE_SIZE,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::rem::rem0rec::{
    REC_N_EXTRA_BYTES, REC_STATUS_INFIMUM, REC_STATUS_SUPREMUM, Record,
};
use crate::ut::ut0dbg::{ut_a, ut_ad};
use crate::utils::TxnId;

pub type SlotId = usize;

/* PAGE HEADER
Index page header starts at the first offset left free by the fil-module.
 */

/*
/***********************************************************************
 *                     InnoDB Page Header (56 bytes)                   *
 *                     Offset: 38 (0x26) from page start               *
 ***********************************************************************
 * Offset | Size | Name              | Description                     *
 *--------+------+-------------------+---------------------------------*
 * 0x26   |  2   | PAGE_N_DIR_SLOTS  | Number of directory slots       *
 * 0x28   |  2   | PAGE_HEAP_TOP     | Offset to start of free space   *
 * 0x2A   |  2   | PAGE_N_HEAP       | Heap records + deleted count    *
 * 0x2C   |  2   | PAGE_FREE         | Start of record free list       *
 * 0x2E   |  2   | PAGE_GARBAGE      | Fragmented space in bytes       *
 * 0x30   |  2   | PAGE_LAST_INSERT  | Offset of last insert           *
 * 0x32   |  2   | PAGE_DIRECTION    | Insert direction (0–2)          *
 * 0x34   |  2   | PAGE_N_DIRECTION  | Count of inserts in direction   *
 * 0x36   |  2   | PAGE_N_RECS       | Number of user records          *
 * 0x38   |  8   | PAGE_MAX_TRX_ID   | Purge-related trx ID (since 5.1)*
 * 0x40   |  8   | PAGE_HEADER_EXTRA | Additional metadata (if used)   *
 ***********************************************************************/

*/

/** Index page header starts at this offset. */
pub const PAGE_HEADER: usize = FSEG_PAGE_DATA;

/** Number of slots in the page-directory. */
pub const PAGE_N_DIR_SLOTS: usize = 0;

/** Pointer to the record heap top. Points to the first unused space (more like never used before).  */
pub const PAGE_HEAP_TOP: usize = 2;

/** Number of records in the heap. */
pub const PAGE_N_HEAP: usize = 4;

/** Pointer to the start of the free record list. This points to the single linked list of free-ed up records. */
pub const PAGE_FREE: usize = 6;

/** Number of bytes in deleted records. */
pub const PAGE_GARBAGE: usize = 8;

/** Pointer to the last inserted record,
or NULL if this information has been reset by a delete operation */
pub const PAGE_LAST_INSERT: usize = 10;

/** Last insert direction. Values: [PAGE_LEFT, PAGE_RIGHT, PAGE_SAME_REC, ..] */
pub const PAGE_DIRECTION: usize = 12;

/** Number of consecutive inserts to the same direction */
pub const PAGE_N_DIRECTION: usize = 14;

/** Number of user records on the page */
pub const PAGE_N_RECS: usize = 16;

/** Highest id of the transaction that may have modified a record on the page; a i64;
defined only in the secondary indexes; NOTE:
this may be modified only when the thread has an x-latch to the page. */
pub const PAGE_MAX_TRX_ID: usize = 18;

/** Level of the node in an index tree.
The leaf is at level 0.
This field should not be written to the page after creation. */
pub const PAGE_LEVEL: usize = 26;

/** IndexId of the page. This field should not be written to the page after creation. */
pub const PAGE_INDEX_ID: usize = 28;

/** File-segment-header of the leaf pages in the btree.
It is defined only on the root page of the BTree. */
pub const PAGE_BTR_SEG_LEAF: usize = 36;

/** Start of data on the page */
pub const PAGE_DATA: usize = PAGE_HEADER + 36 + 2 * FSEG_PAGE_DATA;

/** Offset of the page Infimum record on a page */
pub const PAGE_INFIMUM: usize = PAGE_DATA + 1 + REC_N_EXTRA_BYTES;

/** Offset of the page Supremum record on a page */
pub const PAGE_SUPREMUM: usize =
    PAGE_DATA + 1 + REC_N_EXTRA_BYTES + 8 + 1 + REC_N_EXTRA_BYTES;

/** Offset of the page Supremum record end on the page */
pub const PAGE_SUPREMUM_END: usize = PAGE_SUPREMUM + 9;

/*---------------------------------------------------------*/

/* Heap numbers */
/* Each record stored in InnoDB has a heap number. */

/** Page Infimum */
pub const PAGE_HEAP_NO_INFIMUM: usize = 0;

/** Page Supremum */
pub const PAGE_HEAP_NO_SUPREMUM: usize = 1;

/** First user record in creation (insertion) order. */
pub const PAGE_HEAP_NO_USER_LOW: usize = 2;

/* Directions of cursor movement */
pub const PAGE_LEFT: usize = 0;
pub const PAGE_RIGHT: usize = 1;
pub const PAGE_SAME_REC: usize = 2;
pub const PAGE_SAME_PAGE: usize = 3;
pub const PAGE_NO_DIRECTION: usize = 4;

/// The maximum and the minimum number of records owned by a directory slot.
/// The number may drop below the minimum in the first and last slot in the directory.
pub const PAGE_DIR_SLOT_MAX_N_OWNED: usize = 8;
pub const PAGE_DIR_SLOT_MIN_N_OWNED: usize = 4;

/*	PAGE DIRECTORY */

/// Offset of the directory starts from the page end.
/// We call the slot with the highest file address directory start
/// as it points to the first record in the list.
pub const PAGE_DIR: usize = FIL_PAGE_DATA_END;

/// We define a slot in the page directory as two bytes.
pub const PAGE_DIR_SLOT_SIZE: usize = 2;

#[derive(Copy, Clone)]
pub struct Page {
    ptr: *mut u8,
}

impl Page {
    /// Creates a new Page instance.
    ///
    /// # Arguments
    ///
    /// * `ptr`: A raw pointer to the page data.
    ///
    /// # Returns
    ///
    /// * A new `Page` instance.
    pub fn new(ptr: *mut u8) -> Self {
        Page { ptr }
    }

    pub fn page_rec_check(&self, rec: Record) -> bool {
        ut_ad!(
            self.page_offset(rec) <= self.page_header_get_field(PAGE_HEAP_TOP)
        );
        ut_ad!(self.page_offset(rec) >= PAGE_DATA);

        true
    }

    /// Get the tablespace identifier of the page.
    ///
    /// # Returns
    ///
    /// * The tablespace identifier as a `SpaceId`.
    pub fn page_get_space_id(&self) -> SpaceId {
        unsafe {
            let ptr = self.ptr.add(FIL_PAGE_SPACE_ID) as *const u32;
            ptr.read_unaligned() as SpaceId
        }
    }

    /// Copy the page to a destination buffer.
    pub fn copy_to(&self, buffer: &mut [u8; UNIV_PAGE_SIZE]) {
        let src = self.page_get_as_mut_slice();
        ut_a!(src.len() == buffer.len());

        let src_slice = unsafe { &*src };
        buffer.copy_from_slice(src_slice);
    }

    /// Set the tablespace identifier of the page.
    ///
    /// # Arguments
    ///
    /// * `space_id`: The tablespace identifier to set as a `SpaceId`.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn page_set_space_id(&self, space_id: SpaceId) {
        unsafe {
            let ptr = self.ptr.add(FIL_PAGE_SPACE_ID) as *mut u32;
            ptr.write_unaligned(space_id as u32);
        }
    }

    /// Get the file page offset of the page.
    ///
    /// # Returns
    ///
    /// * The file page offset as a `usize`.
    pub fn page_get_fil_offset(&self) -> usize {
        unsafe {
            let ptr = self.ptr.add(FIL_PAGE_OFFSET) as *const u32;
            ptr.read_unaligned() as usize
        }
    }

    /// Write a new 4-byte page-offset field (little-endian).
    pub fn page_set_fil_offset(&mut self, new_offset: usize) {
        let v: u32 = new_offset as u32;
        let le = v.to_le_bytes();
        unsafe {
            let dst = self.ptr.add(FIL_PAGE_OFFSET);
            dst.copy_from_nonoverlapping(le.as_ptr(), 4);
        }
    }

    /// Get the page data as a mutable vector.
    ///
    /// # Returns
    ///
    /// * A mutable reference to the page data as a `Vec<u8>`.
    pub fn page_get_as_mut_slice(&self) -> *mut [u8] {
        slice_from_raw_parts_mut(self.ptr, UNIV_PAGE_SIZE)
    }

    pub fn to_page_frame(&self) -> PageFrame {
        unsafe {
            let array_ptr = self.ptr as *mut [u8; UNIV_PAGE_SIZE];
            PageFrame(&mut *array_ptr)
        }
    }

    /// Wraps a mutable page‐sized buffer as a Page.
    /// NOTE: Currently it is only used for unit tests.
    pub fn from_buffer(buf: &mut [u8]) -> Self {
        assert_eq!(
            buf.len(),
            UNIV_PAGE_SIZE,
            "Page::from_buffer requires exactly {} bytes",
            UNIV_PAGE_SIZE
        );
        Page {
            ptr: buf.as_mut_ptr(),
        }
    }

    /// Set the max transaction id on the page.
    ///
    /// # Arguments
    ///
    /// * `max_trx_id`: The maximum transaction id to set.
    /// * `mtr`: An optional transaction object.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn set_max_trx_id(&self, max_trx_id: TxnId, mtr: Option<Mtr>) {
        let page = self.ptr;
        /* It is not necessary to write this change to the redo log as
        during the database recovery we assume that the max transaction id
        of every pae is the maximum transaction id assigned before the crash.
        */
        match mtr {
            None => unsafe {
                let loc =
                    page.add(PAGE_HEADER + PAGE_MAX_TRX_ID) as *mut TxnId;

                loc.write_unaligned(max_trx_id);
            },
            Some(_) => {
                warn!("don't know how to handle Mtr");
                panic!("don't know how to handle Mtr");
            }
        }
    }

    /// Retrieves the maximum transaction ID stored in the page.
    /// # Returns
    /// The maximum transaction ID (`TxnId`) stored in the page.
    pub fn get_max_trx_id(&self) -> TxnId {
        unsafe {
            let page_ptr =
                self.ptr.add(PAGE_HEADER + PAGE_MAX_TRX_ID) as *const TxnId;
            page_ptr.read_unaligned()
        }
    }

    /// Calculates the free space available in an empty page.
    ///
    /// # Returns
    /// The amount of free space in bytes.
    pub fn page_get_free_space_of_empty() -> usize {
        // UNIV_PAGE_SIZE = Page Size
        // PAGE_SUPREMUM_END = Offset of the supremum record end.
        // This is where everything's related to the header ends.
        // PAGE_DIR = Size of the page trailer.
        // 2 * PAGE_DIR_SLOT_SIZE = Both infimum and supremum records are stored in the page directory in an empty page as well.
        UNIV_PAGE_SIZE - PAGE_SUPREMUM_END - PAGE_DIR - 2 * PAGE_DIR_SLOT_SIZE
    }

    /// Retrieves the infimum record from the page.
    ///
    /// # Returns
    /// A `Record` object representing the infimum record.
    ///
    pub fn page_get_infimum_rec(&self) -> Record {
        let ptr = unsafe { self.ptr.add(PAGE_INFIMUM) };
        info!("ptr: {}", ptr as usize);
        Record::new(ptr)
    }

    /// Retrieves the supremum record from the page.
    ///
    /// # Returns
    /// A `Record` object representing the supremum record.
    ///
    pub fn page_get_supremum_rec(&self) -> Record {
        let ptr = unsafe { self.ptr.add(PAGE_SUPREMUM) };
        Record::new(ptr)
    }

    /// Sets the value of a given field in the page header.
    ///
    /// # Parameters
    /// - `field`: The offset (in `usize`) of the specific field within the page header.
    /// - `value`: The value to set for the field.
    ///
    /// # Returns
    /// - None
    pub fn page_header_set_field(&self, field: usize, value: usize) {
        unsafe {
            let ptr = self.ptr.add(PAGE_HEADER + field) as *mut u16;
            ptr.write_unaligned(value as u16);
        }
    }

    /// Retrieves the value of a given field from the page header.
    ///
    /// # Parameters
    /// - `field`: The offset (in `usize`) of the specific field within the page header.
    ///
    /// # Returns
    /// - Returns the value of the field as a `usize`.
    pub fn page_header_get_field(&self, field: usize) -> usize {
        unsafe {
            let ptr = self.ptr.add(PAGE_HEADER + field) as *const u16;
            ptr.read_unaligned() as usize
        }
    }

    pub unsafe fn page_header_get_ptr(&self, field: usize) -> *mut u8 {
        unsafe { self.ptr.add(self.page_header_get_field(field)) }
    }

    /// Set the header of the free list.
    ///
    /// # Arguments
    ///
    /// * `record`: The record to set as the head of the free list.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn page_header_set_page_free(&self, record: Record) {
        let offset = self.page_offset(record);
        unsafe {
            let ptr = self.ptr.add(PAGE_HEADER + PAGE_FREE) as *mut u16;
            ptr.write_unaligned(offset as u16)
        }
    }

    /// Get the head of the free list.
    ///
    /// # Returns
    ///
    /// * `Option<Record>`: The head record of the free list if it exists, or `None` if the free list is empty.
    pub fn page_header_get_page_free(&self) -> Option<Record> {
        let offset = self.page_header_get_field(PAGE_FREE);
        if offset == 0 {
            None
        } else {
            unsafe { Some(Record::new(self.ptr.add(offset))) }
        }
    }

    /// Retrieves the number of records in the page.
    ///
    /// # Returns
    ///
    /// * The number of records in the page as a `usize`.
    pub fn page_get_n_recs(&self) -> usize {
        self.page_header_get_field(PAGE_N_RECS)
    }

    /// Allocates memory from the page heap.
    ///
    /// # Arguments
    ///
    /// * `need`: The amount of memory needed in bytes.
    ///
    /// # Returns
    ///
    /// * This function currently has no implementation and returns nothing.
    pub fn page_mem_alloc_heap(&self, need: usize) -> Option<*mut u8> {
        let available_space = self.page_get_max_insert_size(1);

        if available_space >= need {
            unsafe {
                let block = self.page_header_get_ptr(PAGE_HEAP_TOP);

                /* 1. Update the page header to point to the new heap top.  */
                let new_heap_top =
                    self.page_header_get_field(PAGE_HEAP_TOP) + need;
                self.page_header_set_field(PAGE_HEAP_TOP, new_heap_top);

                /* 2. Update the heap no in the page header. */
                let heap_no = self.page_dir_get_n_heap();
                self.page_dir_set_n_heap(heap_no + 1);

                return Some(block);
            }
        }

        None
    }

    /// Checks if the current page is a leaf node.
    ///
    /// # Returns
    ///
    /// * `true` - If the page level is 0, indicating the page is a leaf node.
    /// * `false` - If the page level is greater than 0, indicating the page is not a leaf node.
    pub fn page_is_leaf(&self) -> bool {
        let level = self.page_header_get_field(PAGE_LEVEL);
        level == 0
    }

    /// Retrieves the middle record from the page.
    ///
    /// This method calculates the middle position based on the number of records
    /// and then traverses the records to find the one at that position.
    ///
    /// # Returns
    ///
    /// * A `Record` object representing the middle record in the page.
    pub fn page_get_middle_rec(&self) -> Option<Record> {
        // todo: this function only works with pages with more than 1 record. Why not 1?
        let n_recs = self.page_n_get_recs();
        if n_recs == 0 {
            return None;
        }

        let middle = (n_recs + PAGE_HEAP_NO_USER_LOW) / 2;

        let mut i: usize = 0;
        let mut count: usize = 0;
        loop {
            let n_owned = self.page_dir_get_n_owned(i) as usize;

            if (count + n_owned) > middle {
                break;
            }

            count += n_owned;
            i += 1;
        }

        ut_ad!(i > 0);

        let mut rec = self.page_dir_slot_get_rec(i - 1);
        for _ in 0..(middle - count) {
            let next = rec.rec_get_next();
            rec = next.unwrap();
        }

        Some(rec)
    }

    /// Sets the record pointer in the page directory slot.
    ///
    /// # Parameters
    /// - `slot`: The slot number in the page directory.
    /// - `rec`: The record to set in the slot.
    /// # Returns
    /// - None
    pub fn page_dir_slot_set_rec(&self, slot: usize, rec: Record) {
        let record_offset = self.page_offset(rec);
        unsafe {
            let slot_ptr = self.page_dir_get_nth_slot(slot) as *mut u16;
            slot_ptr.write_unaligned(record_offset as u16);
        }
    }

    /// Get the record pointer to by the slot in the page directory.
    ///
    /// # Parameters
    /// - `slot`: The slot number in the page directory.
    ///
    /// # Returns
    /// - A `Record` object representing the record at the specified slot.
    ///
    pub fn page_dir_slot_get_rec(&self, slot: usize) -> Record {
        unsafe {
            let slot_ptr = self.page_dir_get_nth_slot(slot) as *const u16;
            Record::new(self.ptr.add(slot_ptr.read_unaligned() as usize))
        }
    }

    /// Add a slot to the page directory.
    /// But it does not set the record pointers in the added slots or update n_owned values.
    /// The caller is responsible for the change.
    ///
    pub fn page_dir_add_slot(&self, slot: SlotId) {
        let n_slots = self.page_dir_get_n_slots();

        ut_ad!(slot < n_slots);

        /* Update the page header */
        self.page_dir_set_n_slots(n_slots + 1);

        let slot_ptr = self.page_dir_get_nth_slot(n_slots);

        /* Move slots up */
        unsafe {
            slot_ptr.copy_from(
                slot_ptr.add(PAGE_DIR_SLOT_SIZE),
                (n_slots - 1 - slot) * PAGE_DIR_SLOT_SIZE,
            );
        }
    }

    /// Splits a directory slot that owns too many records.
    ///
    /// # Arguments
    ///
    /// `slot`: The slot to split.
    ///
    pub fn page_dir_split_slot(&self, slot: SlotId) {
        ut_ad!(slot > 0);

        let n_owned = self.page_dir_get_n_owned(slot);
        ut_ad!(n_owned == PAGE_DIR_SLOT_MAX_N_OWNED + 1);

        /* 1. We loop
        to find a record approximately in the middle of the records owned by the slot. */
        let prev_slot = slot - 1;
        let mut rec = self.page_dir_slot_get_rec(prev_slot);

        for i in 0..(n_owned / 2) {
            rec = rec.rec_get_next().unwrap();
        }

        /* 2. Add a slot immediately below the slot to be split. */
        self.page_dir_add_slot(slot - 1);

        /* The added slot is now the slot, and the old slot is slot + 1. */
        let new_slot = slot;

        /* 3. Store appropriate values in the new slot. */
        self.page_dir_slot_set_rec(new_slot, rec);
        self.page_dir_set_n_owned(new_slot, n_owned / 2);

        /* 4. Finally, we update the number of record-field of the original slot.*/
        self.page_dir_set_n_owned(slot + 1, n_owned - (n_owned / 2));
    }

    /// Retrieves the number of records in the page.
    ///
    /// # Returns
    ///
    /// - The number of records in the page as a `usize`.
    ///
    pub fn page_n_get_recs(&self) -> usize {
        self.page_header_get_field(PAGE_N_RECS)
    }

    /// Retrieves the number of slots in the page directory.
    ///
    /// # Returns
    ///
    /// - The number of slots in the page directory as a `SlotId`.
    pub fn page_dir_get_n_slots(&self) -> usize {
        self.page_header_get_field(PAGE_N_DIR_SLOTS)
    }

    /// Sets the number of slots in the page directory.
    ///
    pub fn page_dir_set_n_slots(&self, n_slots: usize) {
        self.page_header_set_field(PAGE_N_DIR_SLOTS, n_slots)
    }

    /// Retrieves the number of records owned by a specific slot in the page directory.
    ///
    /// # Arguments
    ///
    /// * `slot`: The slot ID to get the number of owned records for.
    ///
    /// # Returns
    ///
    /// * The number of records owned by the specified slot as a `SlotId`.
    pub fn page_dir_get_n_owned(&self, slot: SlotId) -> usize {
        let rec = self.page_dir_slot_get_rec(slot);
        rec.rec_get_n_owned()
    }

    /// Sets the number of records owned by a specific slot in the page directory.
    ///
    /// This method updates the number of records that a particular slot in the
    /// page directory owns. This is important for maintaining the balance of
    /// records across directory slots.
    ///
    /// # Parameters
    /// * `slot` - The slot ID to set the number of owned records for
    /// * `n_owned` - The new number of records owned by the slot
    pub fn page_dir_set_n_owned(&self, slot: SlotId, n_owned: usize) {
        let rec = self.page_dir_slot_get_rec(slot);
        rec.rec_set_n_owned(n_owned);
    }

    /// Finds the owner slot of a given record in the page directory.
    ///
    /// # Parameters
    ///
    /// - `record`: The record to find the owner slot for.
    ///
    /// # Returns
    ///
    /// - The slot ID of the owner slot as a `SlotId`.
    ///
    pub fn page_dir_find_owner_slot(&self, record: Record) -> SlotId {
        let first_slot = self.page_dir_get_nth_slot(0);
        let record_offset = self.page_offset(record);

        unsafe {
            let mut slot =
                self.page_dir_get_nth_slot(self.page_dir_get_n_slots() - 1);

            info!("slot: {}", slot as usize);
            info!("record_offset: {}", record_offset);

            while (slot as *mut u16).read_unaligned() as usize != record_offset
            {
                slot = slot.add(PAGE_DIR_SLOT_SIZE);
            }

            (first_slot as usize - slot as usize) / PAGE_DIR_SLOT_SIZE
        }
    }

    /// Retrieves the pointer to the nth slot in the page directory.
    ///
    /// # Parameters
    ///
    /// - `slot_id`: The slot ID of the directory slot.
    ///
    /// # Returns
    ///
    /// - A pointer to the nth slot in the page directory.
    ///
    fn page_dir_get_nth_slot(&self, slot_id: SlotId) -> *mut u8 {
        unsafe {
            self.ptr.add(
                UNIV_PAGE_SIZE
                    - PAGE_DIR
                    - ((slot_id + 1) * PAGE_DIR_SLOT_SIZE),
            )
        }
    }

    /// Get the number of records in the heap.
    ///
    /// This method retrieves the number of records in the heap by reading the PAGE_N_HEAP
    /// field from the page header and applying a bitmask to get only the lower 15 bits.
    ///
    /// # Returns
    ///
    /// * The number of records in the heap as a `usize`.
    pub fn page_dir_get_n_heap(&self) -> usize {
        self.page_header_get_field(PAGE_N_HEAP) & 0x7fff
    }

    /// Sets the number of records in the heap.
    ///
    /// This method sets the number of records in the heap while preserving
    /// the high bit (0x8000) of the previous value.
    ///
    /// # Arguments
    ///
    /// * `n_recs`: The new number of records to set.
    ///
    /// # Returns
    ///
    /// * This function does not return a value.
    pub fn page_dir_set_n_heap(&self, n_recs: usize) {
        let prev_val = self.page_dir_get_n_heap();
        self.page_header_set_field(PAGE_N_HEAP, n_recs | (0x8000 & prev_val));
    }

    /// Calculates the space reserved for directory slots of a given number of records.
    /// The exact value is a fraction number (n * PAGE_DIR_SLOT_SIZE) / PAGE_DIR_SLOT_MIN_N_OWNED,
    /// and it rounded upwards to an integer.
    ///
    /// # Arguments
    ///
    /// * `n_recs`: The number of records for which to calculate the reserved space.
    ///
    /// # Returns
    ///
    /// * The amount of space reserved for directory slots in bytes.
    pub fn page_dir_calc_reserved_space(n_recs: usize) -> usize {
        (PAGE_DIR_SLOT_SIZE * n_recs + PAGE_DIR_SLOT_MIN_N_OWNED - 1)
            / PAGE_DIR_SLOT_MIN_N_OWNED
    }

    /// Create a B-tree index page.
    ///
    /// # Parameters
    ///
    /// - `index`: The index to which the page belongs.
    /// - `buf`: The buffer block associated with the page.
    /// - `mtr`: The transaction object.
    ///
    /// # Returns
    ///
    /// - A `Page` object representing the created page.
    ///
    pub fn page_create(
        index: &Index,
        buf: &BufBlock,
        mtr: Option<Mtr>,
    ) -> Page {
        let ptr = buf.get_frame();
        let page = Page::new(ptr);

        ut_ad!(
            page.ptr as usize % UNIV_PAGE_SIZE == 0,
            "page needs to be aligned to UNIV_PAGE_SIZE"
        );

        if !mtr.is_none() {
            page.page_create_write_log(buf, mtr.unwrap());
        }

        /* CREATE THE INFIMUM AND SUPREMUM RECORDS */

        // Create tuple for infimum record
        let mut infimum_d_tuple = DTuple::new(1);
        infimum_d_tuple.dtuple_set_info_bits(REC_STATUS_INFIMUM);

        let infimum_field = infimum_d_tuple.dtuple_get_nth_field_mut(0);

        infimum_field.dfield_set_data(
            "infimumr".as_bytes().to_vec(),
            8, /* size of infimum string*/
        );
        infimum_field.set_type(DATA_VARCHAR, DATA_ENGLISH | DATA_NOT_NULL, 8);

        let heap_top = unsafe { ptr.add(PAGE_INFIMUM) };
        info!("heap_top: {}", heap_top as usize);
        let infimum_rec = Record::new(heap_top).rec_convert_dtuple_to_rec(
            index,
            &infimum_d_tuple,
            0,
        );
        ut_ad!(infimum_rec.rec_get_n_fields() == 1);
        info!("infimum position: {}", infimum_rec.get_ptr() as usize);
        infimum_rec.rec_set_n_owned(1);
        // infimum_rec.rec_set_heap_no(0);

        // Create tuple for supremum record

        let mut supremum_tuple = DTuple::new(1);
        supremum_tuple.dtuple_set_info_bits(REC_STATUS_SUPREMUM);

        let supremum_field = supremum_tuple.dtuple_get_nth_field_mut(0);
        supremum_field.dfield_set_data(
            "supremumr".as_bytes().to_vec(),
            9, /* size of supremum string*/
        );
        supremum_field.set_type(DATA_VARCHAR, DATA_ENGLISH | DATA_NOT_NULL, 9);

        let heap_top = unsafe { ptr.add(PAGE_SUPREMUM) };
        let supremum_rec = Record::new(heap_top).rec_convert_dtuple_to_rec(
            index,
            &supremum_tuple,
            0,
        );
        ut_ad!(supremum_rec.rec_get_n_fields() == 1);
        info!("supremum position: {}", supremum_rec.get_ptr() as usize);
        supremum_rec.rec_set_n_owned(1);
        // supremum_rec.rec_set_heap_no(1);

        /* 4. INITIALIZE THE PAGE */
        page.page_header_set_field(PAGE_N_DIR_SLOTS, 2);
        page.page_header_set_field(PAGE_HEAP_TOP, PAGE_SUPREMUM_END);
        page.page_header_set_field(PAGE_N_HEAP, PAGE_HEAP_NO_USER_LOW);
        page.page_header_set_field(PAGE_FREE, 0);
        page.page_header_set_field(PAGE_GARBAGE, 0);
        page.page_header_set_field(PAGE_LAST_INSERT, u16::MAX as usize);
        page.page_header_set_field(PAGE_DIRECTION, PAGE_NO_DIRECTION);
        page.page_header_set_field(PAGE_N_DIRECTION, 0);
        page.page_header_set_field(PAGE_N_RECS, 0);
        page.set_max_trx_id(0, None);

        // // Do I need to perform memset here?

        /* 5. SET POINTERS IN RECORDS AND DIR SLOTS */

        // Set the slots to point to the infimum and supremum records.
        page.page_dir_slot_set_rec(0, infimum_rec);
        page.page_dir_slot_set_rec(1, supremum_rec);

        info!(
            "infimum position: {}",
            page.page_get_infimum_rec().get_ptr() as usize
        );
        info!(
            "supremum position: {}",
            page.page_get_supremum_rec().get_ptr() as usize
        );
        // Set the infimum record to point to the supremum record.
        page.page_get_infimum_rec().rec_set_next_offs(PAGE_SUPREMUM);
        // Set the supremum record to point to the infimum record.
        page.page_get_supremum_rec().rec_set_next_offs(0);

        page
    }

    /// Get the offset of the record within the page.
    ///
    /// # Parameters
    ///
    /// - `rec`: The record to get the offset for.
    ///
    /// # Returns
    ///
    /// - The offset of the record within the page as a `usize`.
    fn page_offset(&self, rec: Record) -> usize {
        rec.get_ptr() as usize % UNIV_PAGE_SIZE
    }

    /// Checks if the given record is the infimum record of the page.
    ///
    /// # Parameters
    ///
    /// - `rec`: The record to check.
    ///
    /// # Returns
    ///
    /// - `true`: If the record is the infimum record.
    /// - `false`: If the record is not the infimum record.
    ///
    pub fn page_rec_is_infimum(&self, rec: Record) -> bool {
        let rec_offset = self.page_offset(rec);
        rec_offset == PAGE_INFIMUM
    }

    /// Checks if the given record is the supremum record of the page.
    ///
    /// # Parameters
    ///
    /// - `rec`: The record to check.
    ///
    /// # Returns
    ///
    /// - `true`: If the record is the supremum record.
    /// - `false`: If the record is not the supremum record.
    ///
    pub fn page_rec_is_supremum(&self, rec: Record) -> bool {
        let rec_offset = self.page_offset(rec);

        rec_offset == PAGE_SUPREMUM
    }

    /// Checks if the given record is a user record.
    ///
    /// # Parameters
    ///
    /// - `rec`: The record to check.
    ///
    /// # Returns
    ///
    /// - `true`: If the record is a user record.
    /// - `false`: If the record is not a user record.
    ///
    pub fn page_rec_is_user_rec(&self, rec: Record) -> bool {
        let rec_offset = self.page_offset(rec);

        rec_offset != PAGE_INFIMUM && rec_offset != PAGE_SUPREMUM
    }

    /// Searches for a record within a page that corresponds to the given heap number.
    ///
    /// This method starts at the infimum record of the page and traverses the record list
    /// sequentially until it finds a record with a matching `heap_no`
    /// or reaches the supremum record.
    ///
    /// # Parameters
    ///
    /// - `heap_no`: The heap number of the record to find.
    ///
    /// # Returns
    ///
    /// - `Some(Record)`: If a record with the specified `heap_no` is found.
    /// - `None`: If no record with the specified `heap_no` exists within the page.
    pub fn page_find_rec_with_heap_no(
        &self,
        heap_no: usize,
    ) -> Option<Record> {
        let mut rec = self.page_get_infimum_rec();
        loop {
            let rec_heap_no = rec.rec_get_heap_no();

            if rec_heap_no == heap_no {
                return Some(rec);
            }

            // We have reached the end of the list, and there is nothing left.
            if heap_no == PAGE_HEAP_NO_SUPREMUM {
                return None;
            }

            // Move to the next record
            rec = rec.rec_get_next().unwrap();
        }
    }

    /// Create a log record for the creation of page.
    ///
    /// # Parameters
    ///
    /// - `buf`: The buffer block associated with the page.
    /// - `mtr`: The transaction object.
    ///
    /// # Returns
    ///
    /// * `()`: This function does not return a value.
    pub fn page_create_write_log(&self, buf: &BufBlock, mtr: Mtr) {
        unimplemented!()
    }

    /// Sets the pointer to the next record on the page.
    ///
    /// # Arguments
    ///
    /// * `rec`: The record whose next pointer will be set.
    /// * `next_rec`: An optional record that will be set as the next record. If None, the next pointer will be set to 0.
    ///
    /// # Returns
    ///
    /// * This function does not return a value.
    pub fn page_set_next_rec(&self, rec: Record, next_rec: Option<Record>) {
        ut_ad!(!self.page_rec_is_supremum(rec));
        ut_ad!(next_rec.is_some() && rec != next_rec.unwrap());

        let offset = next_rec.map_or(0, |r| self.page_offset(r));
        rec.rec_set_next_offs(offset);
    }

    /// Add a record to the free list.
    ///
    /// # Arguments
    ///
    /// `rec`: The record to be freed.
    ///
    /// # Returns
    ///
    /// None
    pub fn page_mem_free(&self, rec: Record) {
        // Get the head of the free list. None if empty.
        let free_rec = self.page_header_get_page_free();

        self.page_set_next_rec(rec, free_rec);

        // Make the head of the free list point to the newly freed up record.
        self.page_header_set_page_free(rec);

        // Update the total garbage present on the page.
        let garbage = self.page_header_get_field(PAGE_GARBAGE);
        let total_garbage = garbage + rec.rec_offs_size();
        self.page_header_set_field(PAGE_GARBAGE, total_garbage);

        // Reduce the total number of records present on the page.
        self.page_header_set_field(PAGE_N_RECS, self.page_n_get_recs() - 1);
    }

    /// Calculate the combined maximum size of records that can be inserted on top of the heap record.
    ///
    /// # Arguments
    ///
    /// * `n_recs`: The number of records to consider when calculating the maximum insert size.
    ///
    /// # Returns
    ///
    /// * The maximum size in bytes that can be inserted on top of the heap record.
    pub fn page_get_max_insert_size(&self, n_recs: usize) -> usize {
        info!("heap top={}", self.page_header_get_field(PAGE_HEAP_TOP));
        let occupied = self.page_header_get_field(PAGE_HEAP_TOP)
            - PAGE_SUPREMUM_END
            + Self::page_dir_calc_reserved_space(
                n_recs + self.page_dir_get_n_heap() - 2,
            );

        let free_space = Self::page_get_free_space_of_empty();

        if occupied > free_space { 0 } else { free_space - occupied }
    }

    /// Sets the pointer to the next record on the page.
    pub fn page_rec_set_next(&self, rec: Record, next_rec: Option<Record>) {
        ut_ad!(self.page_rec_check(rec));
        ut_ad!(!self.page_rec_is_supremum(rec));
        ut_ad!(next_rec.is_none() || rec != next_rec.unwrap());

        ut_ad!(
            next_rec.is_none() || self.page_rec_is_infimum(next_rec.unwrap())
        );

        let offset = match next_rec {
            None => 0,
            Some(next) => self.page_offset(next),
        };

        rec.rec_set_next_offs(offset);
    }

    /// Finds the record that owns the given record in the page directory.
    ///
    /// This method traverses the record list starting from the given record
    /// until it finds a record that has a non-zero n_owned value, indicating
    /// that it's an owner record in the page directory.
    ///
    /// # Parameters
    /// * `rec` - The record to find the owner for
    ///
    /// # Returns
    /// * The owner record for the given record
    pub fn page_rec_find_owner_rec(&self, rec: Record) -> Record {
        let mut cur = rec;
        while cur.rec_get_n_owned() == 0 {
            cur = cur.rec_get_next().unwrap();
        }

        cur
    }

    /// Gets the page number of the previous page in the linked list of pages.
    ///
    /// This method retrieves the page number of the previous page in the
    /// doubly-linked list of pages in the tablespace.
    ///
    /// # Returns
    /// * The page number of the previous page as a `PageNo`
    pub fn get_prev_page(&self) -> PageNo {
        unsafe {
            let ptr = self.ptr.add(FIL_PAGE_PREV) as *const u32;
            ptr.read_unaligned()
        }
    }

    /// Gets the page number of the next page in the linked list of pages.
    ///
    /// This method retrieves the page number of the next page in the
    /// doubly linked list of pages in the tablespace.
    ///
    /// # Returns
    /// * The page number of the next page as a `PageNo`
    pub fn get_next_page(&self) -> PageNo {
        unsafe {
            let ptr = self.ptr.add(FIL_PAGE_NEXT) as *const u32;
            ptr.read_unaligned()
        }
    }

    /// Checks if a record needs to be stored externally due to its size.
    ///
    /// This method determines if a record is too large to be stored inline
    /// within a page and should instead be stored externally with only a
    /// pointer in the page. A record is considered too large if its size
    /// exceeds half of the free space available in an empty page.
    ///
    /// # Parameters
    /// * `rec_size` - The size of the record in bytes
    ///
    /// # Returns
    /// * `true` if the record needs to be stored externally, `false` otherwise
    ///
    /// # Panic
    /// Panics in debug mode if the record size is not greater than the extra bytes needed for a record
    pub fn page_rec_needs_ext(rec_size: usize) -> bool {
        ut_ad!(rec_size > REC_N_EXTRA_BYTES);

        rec_size > Self::page_get_free_space_of_empty() / 2
    }

    /// Calculates a checksum for the page data.
    ///
    /// This method computes a checksum value for the page data, which can be used
    /// to verify the integrity of the page. The checksum is calculated based on the
    /// contents of the page and can be used to detect corruption or tampering.
    ///
    /// # Parameters
    /// * `page_frame` - A reference to the page frame containing the data to checksum
    ///
    /// # Returns
    /// * A 32-bit checksum value for the page data
    ///
    /// # Note
    /// This method is currently a placeholder and needs to be implemented.
    pub fn page_data_calc_checksum(page_frame: &PageFrame) -> u32 {
        crc32fast::hash(
            &page_frame.0[FIL_PAGE_OFFSET..UNIV_PAGE_SIZE - FIL_PAGE_DATA],
        )
    }

    /// Get the page header output string for debugging purposes.
    pub fn page_header_str(&self) -> String {
        let mut out = String::new();
        out.push_str(
            &format!(
                "\
--------------------------------\n\
PAGE HEADER INFO\n\
Page address {}, n records {} \n\
n dir slots {}, heap top {}\n\
Page n heap {}, free {}, garbage {}\n\
Page last insert {}, direction {}, n direction {}\n",
                self.ptr as usize,
                self.page_get_n_recs(),
                self.page_header_get_field(PAGE_N_DIR_SLOTS),
                self.page_header_get_field(PAGE_HEAP_TOP),
                self.page_header_get_field(PAGE_N_HEAP),
                self.page_header_get_field(PAGE_FREE),
                self.page_header_get_field(PAGE_GARBAGE),
                self.page_header_get_field(PAGE_LAST_INSERT),
                self.page_header_get_field(PAGE_DIRECTION),
                self.page_header_get_field(PAGE_N_DIRECTION),
            )
            .to_string(),
        );

        out
    }

    /// Print out the header for debugging purposes.
    pub fn page_header_print(&self) {
        let mut out = String::new();
        out.push_str(
            &format!(
                "\
--------------------------------\n\
PAGE HEADER INFO\n\
Page address {}, n records {} \n\
n dir slots {}, heap top {}\n\
Page n heap {}, free {}, garbage {}\n\
Page last insert {}, direction {}, n direction {}\n",
                self.ptr as usize,
                self.page_get_n_recs(),
                self.page_header_get_field(PAGE_N_DIR_SLOTS),
                self.page_header_get_field(PAGE_HEAP_TOP),
                self.page_header_get_field(PAGE_N_HEAP),
                self.page_header_get_field(PAGE_FREE),
                self.page_header_get_field(PAGE_GARBAGE),
                self.page_header_get_field(PAGE_LAST_INSERT),
                self.page_header_get_field(PAGE_DIRECTION),
                self.page_header_get_field(PAGE_N_DIRECTION),
            )
            .to_string(),
        );

        info!("{}", out);
    }
}
