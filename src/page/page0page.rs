use std::{
    ops::Add,
    sync::{Arc, RwLock},
};

use tracing::{info, warn};

use crate::{
    buf::buf0types::BufBlock,
    const_assert,
    data::data0types::{DATA_ENGLISH, DATA_NOT_NULL, DATA_VARCHAR, DTuple},
    dict::dict0types::Index,
    fil::fil0types::{
        FIL_PAGE_DATA, FIL_PAGE_DATA_END, FIL_PAGE_NEXT, FIL_PAGE_OFFSET,
        FIL_PAGE_PREV, FIL_PAGE_SPACE_ID, FSEG_PAGE_DATA,
    },
    innodb::innodb0types::{PageFrame, PageNo, SpaceId, UNIV_PAGE_SIZE},
    mach::mach0data::{
        mach_read_from_2, mach_read_from_4, mach_read_from_8, mach_write_to_2,
        mach_write_to_4, mach_write_to_8,
    },
    mtr::{mtr0mtr::Mtr, mtr0types::MtrLogMode::MTR_LOG_NONE},
    page::page0cur::PageCur,
    rem::rem0rec::{
        REC_N_EXTRA_BYTES, REC_STATUS_INFIMUM, REC_STATUS_SUPREMUM, Record,
    },
    some_or_panic,
    ut::{
        ut0dbg::{ut_a, ut_ad},
        ut0math::ut_align_down,
    },
    ut_panic,
    utils::TxnId,
};

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

    /// Checks if a record belongs to the page and is within valid boundaries.
    ///
    /// # Parameters
    /// * `rec` - The record to check
    ///
    /// # Returns
    /// * `true` if the record is valid, assertions will fail in debug mode if invalid
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
        let page = self.to_page_frame();
        mach_read_from_4(&page.0[FIL_PAGE_SPACE_ID..])
    }

    /// Copy the page to a destination buffer.
    pub fn copy_to(&self, buffer: &mut [u8; UNIV_PAGE_SIZE]) {
        let src = self.to_page_frame();
        ut_a!(src.0.len() == buffer.len());

        buffer.copy_from_slice(src.0);
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
    pub fn page_set_space_id(&mut self, space_id: SpaceId) {
        let page = self.to_page_frame();
        mach_write_to_4(&mut page.0[FIL_PAGE_SPACE_ID..], space_id);
    }

    /// Get the file page offset of the page.
    ///
    /// # Returns
    ///
    /// * The file page offset as a `usize`.
    pub fn page_get_fil_offset(&self) -> usize {
        let page = self.to_page_frame();
        mach_read_from_4(&page.0[FIL_PAGE_OFFSET..]) as usize
    }

    /// Write a new 4-byte page-offset field (little-endian).
    pub fn page_set_fil_offset(&mut self, new_offset: usize) {
        let page = self.to_page_frame();
        mach_write_to_4(&mut page.0[FIL_PAGE_OFFSET..], new_offset as u32);
    }

    /// Converts the page to a PageFrame reference.
    ///
    /// This method creates a PageFrame reference from the page's raw pointer,
    /// allowing access to the page data as a fixed-size array.
    ///
    /// # Returns
    /// * A `PageFrame` reference to the page data
    pub fn to_page_frame(&self) -> PageFrame<'_> {
        unsafe {
            let array_ptr = self.ptr as *mut [u8; UNIV_PAGE_SIZE];
            PageFrame(&mut *array_ptr)
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
    pub fn page_set_max_trx_id(
        &self,
        max_trx_id: TxnId,
        mtr: Option<&mut Mtr>,
    ) {
        /* It is not necessary to write this change to the redo log as
        during the database recovery we assume that the max transaction id
        of every pae is the maximum transaction id assigned before the crash.
        */
        match mtr {
            None => {
                let page = self.to_page_frame();
                mach_write_to_8(
                    &mut page.0[PAGE_HEADER + PAGE_MAX_TRX_ID..],
                    max_trx_id as u64,
                );
            }
            Some(_) => {
                warn!("don't know how to handle Mtr");
                ut_panic!("don't know how to handle Mtr");
            }
        }
    }

    /// Retrieves the maximum transaction ID stored in the page.
    /// # Returns
    /// The maximum transaction ID (`TxnId`) stored in the page.
    pub fn page_get_max_trx_id(&self) -> TxnId {
        let page = self.to_page_frame();
        mach_read_from_8(&page.0[PAGE_HEADER + PAGE_MAX_TRX_ID..])
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
        const_assert!(
            UNIV_PAGE_SIZE
                - PAGE_SUPREMUM_END
                - PAGE_DIR
                - 2 * PAGE_DIR_SLOT_SIZE
                == 16191
        );

        UNIV_PAGE_SIZE - PAGE_SUPREMUM_END - PAGE_DIR - 2 * PAGE_DIR_SLOT_SIZE
    }

    /// Retrieves the infimum record from the page.
    ///
    /// # Returns
    /// A `Record` object representing the infimum record.
    ///
    pub fn page_get_infimum_rec(&self) -> Record {
        let ptr = unsafe { self.ptr.add(PAGE_INFIMUM) };
        Record::new(ptr)
    }

    /// Gets the record from the given offset in the page.
    /// The offset is relative to the start of the page.
    ///
    /// This method creates a new `Record` object from the specified offset.
    ///
    /// # Parameters
    ///
    /// * `offset`: The offset in bytes from the start of the page where the record is located.
    ///
    /// # Returns
    ///
    /// A `Record` object representing the record at the specified offset.
    pub fn page_get_rec(&self, offset: usize) -> Record {
        ut_ad!(offset < UNIV_PAGE_SIZE, "offset: {}", offset);
        Record::new(unsafe { self.ptr.add(offset) })
    }

    /// Checks if the given record belongs to the page.
    ///
    /// This method verifies that the record is stored within this page by comparing
    /// the page-aligned address of the record with the page's address.
    ///
    /// # Parameters
    /// * `record` - The record to check
    ///
    /// # Returns
    /// * `true` if the record belongs to this page, `false` otherwise
    #[cfg(debug_assertions)]
    pub fn is_page_record(&self, record: Record) -> bool {
        record.page_align() == self.ptr
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
        let page = self.to_page_frame();
        mach_write_to_2(&mut page.0[PAGE_HEADER + field..], value as u16);
    }

    /// Retrieves the value of a given field from the page header.
    ///
    /// # Parameters
    /// - `field`: The offset (in `usize`) of the specific field within the page header.
    ///
    /// # Returns
    /// - Returns the value of the field as a `usize`.
    pub fn page_header_get_field(&self, field: usize) -> usize {
        let page = self.to_page_frame();
        mach_read_from_2(&page.0[PAGE_HEADER + field..]) as usize
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
        self.page_header_set_field(PAGE_FREE, self.page_offset(record));
    }

    /// Get the head of the free list.
    ///
    /// # Returns
    ///
    /// * `Option<Record>`: The head record of the free list if it exists, or `None` if the free list is empty.
    pub fn page_header_get_page_free(&self) -> Option<Record> {
        ut_ad!(self.page_validate());

        let offset = self.page_header_get_field(PAGE_FREE);
        if offset == 0 { None } else { Some(self.page_get_rec(offset)) }
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
    /// This method attempts to allocate a block of memory of the specified size from
    /// the page's heap area. It updates the page header to reflect the new heap top
    /// and increments the heap number counter.
    ///
    /// # Arguments
    ///
    /// * `need`: The amount of memory needed in bytes.
    ///
    /// # Returns
    ///
    /// * `Some(usize)`: Offset of the allocated memory block if successful.
    /// * `None`: If there is not enough space available in the page.
    pub fn page_mem_alloc_heap(&self, need: usize) -> Option<usize> {
        let available_space = self.page_get_max_insert_size(1);

        if available_space >= need {
            let block_offset = self.page_header_get_field(PAGE_HEAP_TOP);
            ut_ad!(block_offset < UNIV_PAGE_SIZE);

            /* 1. Update the page header to point to the new heap top.  */
            let new_heap_top =
                self.page_header_get_field(PAGE_HEAP_TOP) + need;
            self.page_header_set_field(PAGE_HEAP_TOP, new_heap_top);
            ut_ad!(self.page_header_get_field(PAGE_HEAP_TOP) != 0);

            /* 2. Update the heap no in the page header. */
            let heap_no = self.page_dir_get_n_heap();
            self.page_dir_set_n_heap(heap_no + 1);

            return Some(block_offset);
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
    pub fn page_dir_slot_set_rec(&self, slot: SlotId, rec: Record) {
        let record_offset = self.page_offset(rec);
        let page = self.to_page_frame();

        let slot_offset = self.page_dir_get_nth_slot(slot);
        mach_write_to_2(&mut page.0[slot_offset..], record_offset as u16);
    }

    /// Get the record pointer to by the slot in the page directory.
    ///
    /// # Parameters
    /// - `slot`: The slot number in the page directory.
    ///
    /// # Returns
    /// - A `Record` object representing the record at the specified slot.
    ///
    pub fn page_dir_slot_get_rec(&self, slot: SlotId) -> Record {
        let slot_offset = self.page_dir_get_nth_slot(slot);
        let page = self.to_page_frame();
        let record_offset = mach_read_from_2(&page.0[slot_offset..]) as usize;

        unsafe { Record::new(self.ptr.add(record_offset)) }
    }

    /// Add a slot to the page directory.
    ///
    /// This method adds a new slot to the page directory at the specified position.
    /// It does not set the record pointers in the added slots or update n_owned values.
    /// The caller is responsible for these changes.
    ///
    /// # Parameters
    /// * `slot` - The position where the new slot should be added
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method only updates the page directory structure and the slot count.
    /// The caller must set appropriate record pointers and ownership values.
    pub fn page_dir_add_slot(&self, slot: SlotId) {
        let n_slots = self.page_dir_get_n_slots();

        ut_ad!(slot < n_slots);

        /* Update the page header */
        self.page_dir_set_n_slots(n_slots + 1);

        let slot_offset = self.page_dir_get_nth_slot(n_slots);
        /* Move slots up */
        let page = self.to_page_frame();

        let low = slot_offset + PAGE_DIR_SLOT_SIZE;
        let high = low + (n_slots - 1 - slot) * PAGE_DIR_SLOT_SIZE;
        page.0.copy_within(low..high, slot_offset);
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
    /// This method updates the page header to reflect the new number of slots
    /// in the page directory. This is typically called when adding or removing
    /// slots from the directory.
    ///
    /// # Parameters
    /// * `n_slots` - The new number of slots to set
    ///
    /// # Returns
    /// * None
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
        let page = self.to_page_frame();
        let first_slot = self.page_dir_get_nth_slot(0);
        let mut rec = record;
        while rec.rec_get_n_owned() == 0 {
            rec = some_or_panic!(rec.rec_get_next());
        }

        let record_offset = self.page_offset(rec);
        let mut slot_offset =
            self.page_dir_get_nth_slot(self.page_dir_get_n_slots() - 1);

        while mach_read_from_2(&page.0[slot_offset..]) as usize
            != record_offset
        {
            slot_offset = slot_offset.add(PAGE_DIR_SLOT_SIZE);
        }

        (first_slot - slot_offset) / PAGE_DIR_SLOT_SIZE
    }

    /// Retrieves the pointer to the nth slot in the page directory.
    ///
    /// # Parameters
    ///
    /// - `slot_id`: The slot ID of the directory slot.
    ///
    /// # Returns
    ///
    /// - Offset to the nth slot in the page directory.
    ///
    pub fn page_dir_get_nth_slot(&self, slot_id: SlotId) -> usize {
        UNIV_PAGE_SIZE - PAGE_DIR - ((slot_id + 1) * PAGE_DIR_SLOT_SIZE)
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
    #[inline(always)]
    pub fn page_dir_calc_reserved_space(n_recs: usize) -> usize {
        (PAGE_DIR_SLOT_SIZE * n_recs + PAGE_DIR_SLOT_MIN_N_OWNED - 1)
            / PAGE_DIR_SLOT_MIN_N_OWNED
    }

    /// Create a B-tree index page.
    ///
    /// This method initializes a new page for use in a B-tree index structure. It performs
    /// several key operations:
    /// 1. Creates and initializes page header
    /// 2. Creates infimum and supremum records that serve as boundaries for the page
    /// 3. Sets up the page directory with initial slots
    /// 4. Establishes the record linkage between infimum and supremum
    /// 5. Logs the page creation operation if a mini-transaction is provided
    ///
    /// The created page is empty except for the system records (infimum and supremum)
    /// and is ready to have user records inserted into it.
    ///
    /// # Parameters
    ///
    /// * `index` - The index to which the page belongs
    /// * `buf` - The buffer block where the page will be created
    /// * `mtr` - Optional mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// * A `Page` object representing the newly created page
    ///
    pub fn page_create(
        index: &Index,
        buf: &BufBlock,
        mtr: Option<&mut Mtr>,
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
        infimum_rec.rec_set_heap_no(0);

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
        supremum_rec.rec_set_heap_no(1);

        /* 4. INITIALIZE THE PAGE */
        page.page_header_set_field(PAGE_N_DIR_SLOTS, 2);
        page.page_header_set_field(PAGE_HEAP_TOP, PAGE_SUPREMUM_END);
        page.page_dir_set_n_heap(PAGE_HEAP_NO_USER_LOW);
        page.page_header_set_field(PAGE_FREE, 0);
        page.page_header_set_field(PAGE_GARBAGE, 0);
        page.page_header_set_field(PAGE_LAST_INSERT, u16::MAX as usize);
        page.page_header_set_field(PAGE_DIRECTION, PAGE_NO_DIRECTION);
        page.page_header_set_field(PAGE_N_DIRECTION, 0);
        page.page_header_set_field(PAGE_N_RECS, 0);
        page.page_set_max_trx_id(0, None);

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
        ut_ad!(self.is_page_record(rec));
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
        ut_ad!(
            ut_align_down(self.ptr as usize, UNIV_PAGE_SIZE)
                == ut_align_down(rec.get_ptr() as usize, UNIV_PAGE_SIZE)
        );

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
    pub fn page_create_write_log(&self, buf: &BufBlock, mtr: &mut Mtr) {
        // todo
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

        self.page_rec_set_next(rec, free_rec);
        self.page_rec_set_next(rec, free_rec);

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
        ut_ad!(self.page_header_get_field(PAGE_HEAP_TOP) != 0);

        let occupied = self.page_header_get_field(PAGE_HEAP_TOP)
            - PAGE_SUPREMUM_END
            + Self::page_dir_calc_reserved_space(
                n_recs + self.page_dir_get_n_heap() - 2,
            );

        let free_space = Self::page_get_free_space_of_empty();

        if occupied > free_space { 0 } else { free_space - occupied }
    }

    /// Sets the pointer to the next record on the page.
    ///
    /// This method updates the next-record pointer of the given record to point to
    /// another record or to null (indicating end of list). This is used to maintain
    /// the linked list of records on the page.
    ///
    /// # Parameters
    /// * `rec` - The record whose next pointer will be updated
    /// * `next_rec` - The record that should be set as the next record, or None to indicate end of list
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method performs several assertions in debug mode to ensure the integrity of the record chain.
    pub fn page_rec_set_next(&self, rec: Record, next_rec: Option<Record>) {
        ut_ad!(self.page_rec_check(rec));
        ut_ad!(!self.page_rec_is_supremum(rec));
        ut_ad!(next_rec.is_none() || rec != next_rec.unwrap());
        ut_ad!(
            next_rec.is_none()
                || self.page_rec_is_infimum(next_rec.unwrap()) == false
        );
        ut_ad!(
            next_rec.is_none()
                || rec.page_align() == next_rec.unwrap().page_align()
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
            cur = some_or_panic!(cur.rec_get_next());
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
        let page = self.to_page_frame();
        mach_read_from_4(&page.0[FIL_PAGE_PREV..])
    }

    /// Gets the page number of the next page in the linked list of pages.
    ///
    /// This method retrieves the page number of the next page in the
    /// doubly linked list of pages in the tablespace.
    ///
    /// # Returns
    /// * The page number of the next page as a `PageNo`
    pub fn get_next_page(&self) -> PageNo {
        let page = self.to_page_frame();
        mach_read_from_4(&page.0[FIL_PAGE_NEXT..])
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

    /// Copies records from the source page to the destination page.
    ///
    /// This method copies records from the beginning of the source page up to (including)
    /// the specified record.
    /// Infimum and Supremum records are not copied.
    ///
    /// # Parameters
    /// * `dest` - Destination buffer block where records will be copied to
    /// * `src` - Source buffer block where records will be copied from
    /// * `rec` - The record up to which copying should occur (exclusive)
    /// * `index` - Index to which the records belong
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// If the destination page is empty (newly created), it will use a specialized method
    /// for copying to an empty page.
    /// For clustered indexes on leaf pages, the maximum
    /// transaction ID is also updated.
    pub fn page_copy_rec_list_start(
        &self,
        dest: Arc<RwLock<BufBlock>>,
        src: Arc<RwLock<BufBlock>>,
        rec: Record,
        index: Arc<Index>,
        mtr: &mut Mtr,
    ) {
        let dest_page = dest.write().unwrap().get_page();
        let src_page = src.write().unwrap().get_page();

        ut_ad!(dest_page.page_is_leaf() == src_page.page_is_leaf());

        if dest_page.page_dir_get_n_heap() == PAGE_HEAP_NO_USER_LOW {
            dest_page.page_copy_rec_list_start_to_created_page(
                dest_page,
                rec,
                index.clone(),
                dest,
                mtr,
            );
        }

        if index.is_clustered() && src_page.page_is_leaf() {
            dest_page.page_update_max_trx_id(self.page_get_max_trx_id(), mtr);
        }
    }

    /// Copies records from source page to the destination page.
    ///
    /// This method copies records from the specified record to the end of the source page.
    /// Infimum and Supremum records are not copied.
    /// The records are copied to the start of the record list on the destination page.
    ///
    /// # Parameters
    /// * `dest` - Destination buffer block where records will be copied to
    /// * `rec` - The starting record from which copying should begin (inclusive)
    /// * `index` - Index to which the records belong
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// If the destination page is empty (newly created), it will use a specialized method
    /// for copying to an empty page.
    /// For clustered indexes on leaf pages, the maximum
    /// transaction ID is also updated.
    pub fn page_copy_rec_list_end(
        &self,
        dest: Arc<RwLock<BufBlock>>,
        rec: Record,
        index: Arc<Index>,
        mtr: &mut Mtr,
    ) {
        ut_ad!(self.is_page_record(rec));

        let dest_page = dest.write().unwrap().get_page();

        if dest_page.page_dir_get_n_heap() == PAGE_HEAP_NO_USER_LOW {
            self.page_copy_rec_list_end_to_created_page(
                dest_page,
                rec,
                index.clone(),
                dest,
                mtr,
            );
        }

        if index.is_clustered() && self.page_is_leaf() {
            dest_page.page_update_max_trx_id(self.page_get_max_trx_id(), mtr);
        }
    }

    /// Moves records from source page to the destination page.
    ///
    /// This function moves records from the current page (source) to a destination page,
    /// starting from the given record and including all subsequent records up to the end
    /// of the page. The function first copies the records to the destination page and then
    /// deletes them from the source page.
    ///
    /// # Parameters
    /// * `dest` - Destination buffer block where records will be moved to
    /// * `rec` - Starting record to be moved (and all records after it)
    /// * `index` - Index to which the records belong
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Notes
    /// * Infimum and Supremum records are not moved
    /// * The function maintains the integrity of the page directory and record chains
    /// * This operation is logged in the mini-transaction
    pub fn page_move_rec_list_end(
        &self,
        dest: Arc<RwLock<BufBlock>>,
        rec: Record,
        index: Arc<Index>,
        mtr: &mut Mtr,
    ) {
        let dest_page = dest.read().unwrap().get_page();

        let old_n_recs = dest_page.page_get_n_recs();
        let old_page_size = dest_page.page_get_data_size();

        self.page_copy_rec_list_end(dest, rec, index, mtr);

        let new_n_recs = dest_page.page_get_n_recs();
        let new_page_size = dest_page.page_get_data_size();

        ut_ad!(new_page_size > old_page_size);

        self.page_delete_rec_list_end(
            rec,
            Some(new_n_recs - old_n_recs),
            Some(new_page_size - old_page_size),
            mtr,
        );
    }

    /// Deletes records from the page starting from the beginning up to the given record.
    ///
    /// This method removes records from the page, starting from the first user record
    /// (after infimum) and continuing up to (but not including) the specified record.
    /// It uses a page cursor to navigate and delete each record individually.
    ///
    /// # Parameters
    /// * `rec` - The record up to which deletion should occur (exclusive)
    /// * `buf_block` - The buffer block containing the page
    /// * `index` - Index to which the records belong
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// If the specified record is the infimum record, no deletion occurs.
    /// The method temporarily disables logging of individual record deletions
    /// and then restores the original log mode.
    pub fn page_delete_rec_list_start(
        &mut self,
        rec: Record,
        buf_block: Arc<RwLock<BufBlock>>,
        index: Arc<Index>,
        mtr: &mut Mtr,
    ) {
        if self.page_rec_is_infimum(rec) {
            return;
        }

        // page_delete_rec_list_write_log

        let mut page_cur =
            PageCur::new(index, Some(rec), buf_block, self.clone());

        page_cur.page_cur_set_before_first();
        page_cur.page_cur_move_to_next();

        while page_cur.page_cur_get_rec() != rec {
            page_cur.page_cur_delete_rec(Some(mtr));
        }

        // Individual delete records are not logged.
        // todo: Why??
        let log_mode = mtr.set_log_mode(MTR_LOG_NONE);

        // Restore log mode
        let old_mode = mtr.set_log_mode(log_mode);
        ut_ad!(old_mode == MTR_LOG_NONE);
    }

    /// Deletes records from the page starting from the given record to the end of the page.
    ///
    /// This function removes a chain of records from the page, starting from the specified record
    /// and continuing to the end of the page (before the supremum record). It updates the page
    /// directory, record links, and page metadata to maintain consistency after deletion.
    ///
    /// # Parameters
    /// * `rec` - The first record to be deleted (and all records after it)
    /// * `n_recs` - Optional number of records being deleted (if known)
    /// * `size` - Optional total size of records being deleted in bytes (if known)
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Notes
    /// * The Infimum and Supremum records are never deleted
    /// * If the record is the infimum record, deletion starts from the next record
    /// * If the record is the supremum record, no deletion occurs
    /// * The function updates page metadata including garbage size and record count
    /// * This operation is logged in the mini-transaction
    /// * The page directory is updated to maintain proper record ownership
    pub fn page_delete_rec_list_end(
        &self,
        rec: Record,
        n_recs: Option<usize>,
        size: Option<usize>,
        mtr: &mut Mtr,
    ) {
        let rec = if self.page_rec_is_infimum(rec) {
            some_or_panic!(rec.rec_get_next())
        } else {
            rec
        };

        if self.page_rec_is_supremum(rec) {
            return;
        }

        let (deleted_size, deleted_n_recs) =
            if size.is_none() || n_recs.is_none() {
                (0, 0)
            } else {
                (some_or_panic!(size), some_or_panic!(n_recs))
            };

        // 1. Reset the last insert info in the page header.
        self.page_header_set_field(PAGE_LAST_INSERT, 0);

        // 2. Increment the modify clock of the block.
        // buf_block_modify_clock_inc

        // 3. Write the log record.
        // page_delete_rec_list_write_log

        let prev_rec = rec.rec_get_prev();
        let last_rec = self.page_get_supremum_rec().rec_get_prev();

        // Update the page directory.
        let mut count = 0;
        let mut rec2 = rec;
        while rec2.rec_get_n_owned() == 0 {
            count += 1;
            rec2 = some_or_panic!(rec2.rec_get_next());
        }

        ut_ad!(rec2.rec_get_n_owned() > count);
        let n_owned = rec2.rec_get_n_owned() - count;
        let slot_id = self.page_dir_find_owner_slot(rec2);

        self.page_dir_slot_set_rec(slot_id, self.page_get_supremum_rec());
        self.page_dir_set_n_owned(slot_id, n_owned);

        self.page_dir_set_n_slots(slot_id + 1);

        // Remove the record chain segment from the page.
        self.page_rec_set_next(
            some_or_panic!(prev_rec),
            Some(self.page_get_supremum_rec()),
        );

        // Concatenate the record list freed up to the free list.
        self.page_rec_set_next(
            some_or_panic!(last_rec),
            Some(self.page_get_supremum_rec()),
        );

        self.page_header_set_field(
            PAGE_GARBAGE,
            deleted_size + self.page_header_get_field(PAGE_GARBAGE),
        );
        self.page_header_set_field(
            PAGE_N_RECS,
            self.page_header_get_field(PAGE_N_RECS) - deleted_n_recs,
        );
    }

    /// Set the max transaction id to the new value if it exceeds the previous value.
    ///
    /// This method updates the maximum transaction ID stored in the page if the provided
    /// transaction ID is greater than the current maximum. This is used to track the
    /// highest transaction ID that has modified the page.
    ///
    /// # Parameters
    /// * `txn_id` - The transaction ID to potentially set as the new maximum
    /// * `mtr` - Mini-transaction handle for logging the change
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method only operates on leaf pages and will assert in debug mode if called on a non-leaf page.
    pub fn page_update_max_trx_id(&self, txn_id: TxnId, mtr: &mut Mtr) {
        ut_ad!(self.page_is_leaf());

        let mut mtr = mtr;

        if self.page_get_max_trx_id() < txn_id {
            self.page_set_max_trx_id(txn_id, Some(&mut mtr));
        }
    }

    /// Returns the fill factor of the page as a percentage.
    /// This is calculated as the ratio of the remaining space available for inserting records
    /// to the total free space of an empty page, multiplied by 100.
    ///
    /// # Returns
    ///
    /// * The fill factor of the page as a `usize` percentage.
    pub fn page_get_current_fill_factor(&self) -> usize {
        let remaining_size = self.page_get_max_insert_size(1);
        let empty_page_free_space = Self::page_get_free_space_of_empty();

        ((empty_page_free_space - remaining_size) * 100)
            / empty_page_free_space
    }

    /// Returns the fill factor of the page after inserting a record of the given size.
    ///
    /// # Parameters
    ///
    /// * `insert_size`: The size of the record to be inserted.
    ///
    /// # Returns
    ///
    /// * The fill factor of the page after the insertion as a `usize` percentage.
    ///
    pub fn page_get_fill_factor_after_insert(
        &self,
        insert_size: usize,
    ) -> usize {
        let max_insert_size = self.page_get_max_insert_size(1);
        let (remaining_size, overflowed) =
            max_insert_size.overflowing_sub(insert_size);

        if overflowed {
            return 100;
        }

        let empty_page_free_space = Self::page_get_free_space_of_empty();

        ((empty_page_free_space - remaining_size) * 100)
            / empty_page_free_space
    }

    /// Copies records from the start to the given record (exclusive) to a newly created page.
    ///
    /// This specialized method copies records from the beginning of the source page up to
    /// (but not including) the specified record to a newly created (empty) destination page.
    /// It is optimized for the case where the destination page has just been initialized.
    ///
    /// # Parameters
    /// * `dest_page` - Destination page where records will be copied to
    /// * `rec` - The record up to which copying should occur (exclusive)
    /// * `index` - Index to which the records belong
    /// * `buf_block` - The buffer block containing the destination page
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method asserts that the destination page is empty (has the initial heap number).
    /// If the specified record is the infimum record, no copying occurs.
    pub fn page_copy_rec_list_start_to_created_page(
        &self,
        dest_page: Page,
        rec: Record,
        index: Arc<Index>,
        buf_block: Arc<RwLock<BufBlock>>,
        mtr: &mut Mtr,
    ) {
        ut_ad!(dest_page.page_dir_get_n_heap() == PAGE_HEAP_NO_USER_LOW);

        // If the given record is infimum, we don't have to copy anything.
        if self.page_rec_is_infimum(rec) {
            return;
        }

        let mut cur_rec = if self.page_rec_is_infimum(rec) {
            some_or_panic!(rec.rec_get_next())
        } else {
            rec
        };

        // Uncomment
        // page_copy_rec_list_to_created_page_write_log

        let dest_page_cur = PageCur::new(index, None, buf_block, dest_page);
        while cur_rec != rec {
            dest_page_cur.page_cur_insert_rec_low(cur_rec, mtr);
            cur_rec = some_or_panic!(cur_rec.rec_get_next());
        }
    }

    /// Copies records from the specified record to the end of the page to a newly created page.
    ///
    /// This specialized method copies records from the specified record to the end of the source page
    /// (including the specified record) to a newly created (empty) destination page.
    /// It is optimized for the case where the destination page has just been initialized.
    ///
    /// # Parameters
    /// * `dest_page` - Destination page where records will be copied to
    /// * `rec` - The starting record from which copying should begin (inclusive)
    /// * `index` - Index to which the records belong
    /// * `buf_block` - The buffer block containing the destination page
    /// * `mtr` - Mini-transaction handle
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method asserts that the destination page is empty (has the initial heap number).
    /// If the specified record is the infimum record, copying starts from the next record.
    /// If the specified record is the supremum record, no copying occurs.
    pub fn page_copy_rec_list_end_to_created_page(
        &self,
        dest_page: Page,
        rec: Record,
        index: Arc<Index>,
        buf_block: Arc<RwLock<BufBlock>>,
        mtr: &mut Mtr,
    ) {
        ut_ad!(self.is_page_record(rec));
        ut_ad!(dest_page.page_dir_get_n_heap() == PAGE_HEAP_NO_USER_LOW);

        let mut rec = if self.page_rec_is_infimum(rec) {
            some_or_panic!(rec.rec_get_next())
        } else {
            rec
        };

        if self.page_rec_is_supremum(rec) {
            return;
        }

        // Uncomment
        // page_copy_rec_list_to_created_page_write_log

        let mut dest_page_cur =
            PageCur::new(index, None, buf_block, dest_page);
        while self.page_rec_is_supremum(rec) == false {
            dest_page_cur.page_cur_insert_rec_low(rec, mtr);
            dest_page_cur.page_cur_move_to_next();

            rec = some_or_panic!(rec.rec_get_next());
        }
    }

    /// Returns the sum of records in the record list excluding the infimum and supremum records.
    ///
    /// This function calculates the total size of user data stored in the page by subtracting
    /// the space used by system records (infimum and supremum) and garbage from the total
    /// heap space used.
    ///
    /// # Returns
    ///
    /// The total size (in bytes) of user data in the page
    pub fn page_get_data_size(&self) -> usize {
        let ret = self.page_header_get_field(PAGE_HEAP_TOP)
            - PAGE_SUPREMUM_END
            - self.page_header_get_field(PAGE_GARBAGE);

        ut_ad!(ret < UNIV_PAGE_SIZE);

        ret
    }

    /// Returns the maximum combined size of records that can be inserted on top of the record heap if the page is first reorganized.
    ///
    /// This function calculates how much space would be available for new records if the page
    /// were reorganized. It takes into account the current data size, the space needed for
    /// directory slots, and the total free space available in an empty page.
    ///
    /// # Arguments
    ///
    /// * `n_recs` - The number of records to be inserted
    ///
    /// # Returns
    ///
    /// The maximum size (in bytes) available for new records after reorganization,
    /// or 0 if the page would be full
    pub fn page_get_max_insert_size_after_reorganize(
        &self,
        n_recs: usize,
    ) -> usize {
        let occupied = self.page_get_data_size()
            + Self::page_dir_calc_reserved_space(
                n_recs + self.page_get_n_recs(),
            );

        let free_space = Self::page_get_free_space_of_empty();

        if occupied > free_space { 0 } else { free_space - occupied }
    }

    /// Validates the integrity of a page.
    ///
    /// This function performs basic validation checks on the page structure to ensure
    /// its integrity. Currently, it verifies that the page heap top pointer is not zero,
    /// which is a fundamental requirement for a properly initialized page.
    ///
    /// # Returns
    /// * `true` if the page passes all validation checks
    /// * In debug mode, assertions will fail if validation errors are detected
    ///
    /// # Note
    /// This function is important for detecting corruption in the database pages.
    /// Additional validation checks may be added in the future to enhance the
    /// robustness of page integrity verification.
    pub fn page_validate(&self) -> bool {
        ut_ad!(self.page_header_get_field(PAGE_HEAP_TOP) != 0);

        true
    }

    /// Get the page header output string for debugging purposes.
    ///
    /// This method creates a formatted string containing the key information from the page header,
    /// including the page address, record count, directory slots, heap information, and other
    /// page metadata. This is useful for debugging and logging.
    ///
    /// # Returns
    /// * A formatted string containing the page header information
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

    /// Print out the page header information for debugging purposes.
    ///
    /// This method formats the page header information into a string and logs it
    /// using the tracing framework. It includes details such as the page address,
    /// record count, directory slots, heap information, and other page metadata.
    ///
    /// # Returns
    /// * None
    ///
    /// # Note
    /// This method uses the same formatting as `page_header_str()` but outputs
    /// the result to the log instead of returning it as a string.
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
                self.page_dir_get_n_heap(),
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

#[cfg(debug_assertions)]
pub trait PageDebug {
    /// Calculates the total size of all records from the beginning up to and including the record at the specified index.
    ///
    /// This function sums the sizes of all user records from the start of the page up to and including
    /// the record at the given index. It is primarily used for debugging and testing purposes.
    ///
    /// # Arguments
    /// * `rec_index` - The index of the record up to which sizes should be summed
    ///
    /// # Returns
    /// * The total size in bytes of all records from the beginning up to and including the specified index
    ///
    /// # Panics
    /// * If there are no records in the specified range
    fn page_get_rec_size_from_start_to_rec(&self, rec_index: usize) -> usize;

    /// Calculates the total size of all records after the specified index to the end of the page.
    ///
    /// This function sums the sizes of all user records that come after the record at the given index.
    /// It is primarily used for debugging and testing purposes.
    ///
    /// # Arguments
    /// * `rec_index` - The index after which record sizes should be summed
    ///
    /// # Returns
    /// * The total size in bytes of all records after the specified index to the end of the page
    ///
    /// # Panics
    /// * If there are no records in the specified range
    fn page_get_rec_size_from_rec_to_end(&self, rec_index: usize) -> usize;

    /// Returns the index of a record within the page's user records.
    ///
    /// This function finds the position (index) of the given record in the list of user records
    /// on the page.
    /// It is primarily used for testing purposes.
    ///
    /// # Arguments
    /// * `rec` - The record whose index is to be found
    ///
    /// # Returns
    /// * The zero-based index of the record within the page's user records
    ///
    /// # Panics
    /// * If the record is not a user record
    /// * If the record is not found in the page's user records
    fn page_get_rec_index(&self, rec: Record) -> usize;

    /// Retrieves all user records on the page.
    ///
    /// This function collects all user records (excluding infimum and supremum) from the page
    /// into a vector. It is primarily used for debugging and testing purposes.
    ///
    /// # Returns
    /// * A vector of user records (excluding infimum and supremum)
    fn page_get_user_records(&self) -> Vec<Record>;
}

#[cfg(debug_assertions)]
impl PageDebug for Page {
    fn page_get_rec_size_from_start_to_rec(&self, rec_index: usize) -> usize {
        let user_recs = self.page_get_user_records();
        let left_sum = user_recs
            .iter()
            .enumerate()
            .filter(|item| item.0 <= rec_index)
            .map(|item| item.1.rec_offs_size())
            .reduce(|a, b| a + b);

        some_or_panic!(left_sum)
    }

    fn page_get_rec_size_from_rec_to_end(&self, rec_index: usize) -> usize {
        let user_recs = self.page_get_user_records();
        let left_sum = user_recs
            .iter()
            .enumerate()
            .filter(|item| item.0 > rec_index)
            .map(|item| item.1.rec_offs_size())
            .reduce(|a, b| a + b);

        some_or_panic!(left_sum)
    }

    fn page_get_rec_index(&self, rec: Record) -> usize {
        ut_ad!(self.page_rec_is_user_rec(rec));

        some_or_panic!(
            self.page_get_user_records().iter().position(|r| r == &rec)
        )
    }

    fn page_get_user_records(&self) -> Vec<Record> {
        let mut result = Vec::<Record>::new();
        let mut cur_rec = Some(self.page_get_infimum_rec());
        while cur_rec.is_some() {
            let rec = some_or_panic!(cur_rec);
            if self.page_rec_is_user_rec(rec) {
                result.push(rec);
            }

            cur_rec = cur_rec.unwrap().rec_get_next();
        }

        result
    }
}
