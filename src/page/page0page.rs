use crate::buf::buf0types::BufBlock;
use crate::data::data0types::{
    DTuple, DATA_ENGLIGH, DATA_NOT_NULL, DATA_VARCHAR,
};
use crate::dict::dict0types::Index;
use crate::fil::fil0types::{FIL_PAGE_DATA_END, FSEG_PAGE_DATA};
use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
use crate::mtr::mtr0mtr::Mtr;
use crate::rem::rem0rec::{
    Record, REC_N_EXTRA_BYTES, REC_STATUS_INFIMUM, REC_STATUS_SUPREMUM,
};
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

pub const PAGE_SIZE: usize = 16 * 1024;

/*	PAGE DIRECTORY */

/// Offset of the directory starts from the page end.
/// We call the slot with the highest file address directory start
/// as it points to the first record in the list.
pub const PAGE_DIR: usize = FIL_PAGE_DATA_END;

/// We define a slot in the page directory as two bytes.
pub const PAGE_DIR_SLOT_SIZE: usize = 2;

pub(crate) struct Page {
    ptr: *mut u8,
}

impl Page {
    pub fn new(ptr: *mut u8) -> Self {
        Page { ptr }
    }

    /// Set the max transaction id on the page.
    ///
    /// # Arguments
    ///
    /// * `max_trx_id`: The maximum transaction id to set.
    /// * `Mtr`: An optional transaction object.
    ///
    /// Returns: ()
    pub fn set_max_trx_id(&self, max_trx_id: TxnId, mtr: Option<Mtr>) {
        let page = self.ptr;
        /* It is not necessary to write this change to the redo log as
        during the database recovery we assume that the max transaction id
        of every pae is the maximum transaction id assigned before the crash.
        */
        match mtr {
            None => unsafe {
                let loc =
                    page.add((PAGE_HEADER + PAGE_MAX_TRX_ID)) as *mut TxnId;

                loc.write(max_trx_id);
            },
            Some(_) => {
                log::warn!("don't know how to handle Mtr");
                panic!("don't know how to handle Mtr");
            }
        }
    }

    /// Retrieves the maximum transaction ID stored in the page.
    /// # Returns
    /// The maximum transaction ID (`TxnId`) stored in the page.
    pub fn get_max_trx_id(&self) -> TxnId {
        unsafe {
            let page_ptr = (self.ptr as *const TxnId)
                .add((PAGE_HEADER + PAGE_MAX_TRX_ID));
            *page_ptr
        }
    }

    /// Calculates the free space available in an empty page.
    ///
    /// # Returns
    /// The amount of free space in bytes.
    pub fn page_get_free_space_of_empty(&self) -> usize {
        // UNIV_PAGE_SIZE = Page Size
        // PAGE_SUPREMUM_END = Offset of the supremum record end. This is where everythings related to the header ends.
        // PAGE_DIR = Size of the page trailer.
        // 2 * PAGE_DIR_SLOT_SIZE = Both infimum and supremum records are stored in the page directory in an empty page as well.
        return UNIV_PAGE_SIZE
            - PAGE_SUPREMUM_END
            - PAGE_DIR
            - 2 * PAGE_DIR_SLOT_SIZE;
    }

    /// Retrieves the infimum record from the page.
    ///
    /// # Returns
    /// A `Record` object representing the infimum record.
    ///
    pub fn page_get_infimum_rec(&self) -> Record {
        let ptr = unsafe { self.ptr.add(PAGE_HEADER + PAGE_INFIMUM) };
        Record::new(ptr)
    }

    /// Retrieves the supremum record from the page.
    ///
    /// # Returns
    /// A `Record` object representing the supremum record.
    ///    
    pub fn page_get_supremum_rec(&self) -> Record {
        let ptr = unsafe { self.ptr.add(PAGE_HEADER + PAGE_SUPREMUM) };
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
    pub fn page_set_header_field(&self, field: usize, value: usize) {
        unsafe {
            let ptr = self.ptr.add(PAGE_HEADER + field) as *mut u16;
            *ptr = value as u16;
        }
    }

    /// Retrieves the value of a given field from the page header.
    ///
    /// # Parameters
    /// - `field`: The offset (in `usize`) of the specific field within the page header.
    ///
    /// # Returns
    /// - Returns the value of the field as a `usize`.
    pub fn page_get_header_field(&self, field: usize) -> usize {
        unsafe {
            let ptr = self.ptr.add(PAGE_HEADER + field) as *const u16;
            *ptr as usize
        }
    }

    /// Checks if the current page is a leaf node.
    ///
    /// # Returns
    ///
    /// * `true` - If the page level is 0, indicating the page is a leaf node.
    /// * `false` - If the page level is greater than 0, indicating the page is not a leaf node.
    pub fn page_is_leaf(&self) -> bool {
        let level = self.page_get_header_field(PAGE_LEVEL);
        level == 0
    }

    pub fn page_get_middle_rec(&self) -> Record {
        let middle = (self.page_get_header_field(PAGE_N_RECS)
            + PAGE_HEAP_NO_USER_LOW)
            / 2;

        let mut i: usize = 0;
        let mut count: usize = 0;
        loop {
            let n_owned = self.page_dir_get_n_owned(i) as usize;

            if (count + n_owned) >= middle {
                break;
            }

            count += n_owned;
            i += 1;
        }

        let mut rec = self.page_dir_slot_get_rec(i - 1);
        for _ in 0..(middle - count) {
            let next = rec.rec_get_next();
            rec = next;
        }

        rec
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
            let ptr = self.ptr.add(PAGE_HEADER + slot);
            Record::new(ptr)
        }
    }

    pub fn page_n_get_recs(&self) -> usize {
        self.page_get_header_field(PAGE_N_RECS)
    }

    pub fn page_dir_get_n_slots(&self, slot: usize) -> SlotId {
        return 5;
    }

    pub fn page_dir_get_n_owned(&self, slot: usize) -> SlotId {
        let page_dir = unsafe { self.ptr.add(PAGE_HEADER as usize) };
        let slot_ptr = unsafe { page_dir.add(slot as usize) };
        let n_owned = unsafe { *(slot_ptr as *const u16) };
        return n_owned as SlotId;
    }

    pub fn page_dir_find_owner_slot(&self, record: Record) -> SlotId {
        let page_ptr = record.page_align();
        let first_slot = self.page_dir_get_nth_slot(0);

        return 6;
    }

    pub fn page_dir_get_nth_slot(&self, slot_id: SlotId) -> *const u8 {
        let page_dir = unsafe { self.ptr.add(PAGE_HEADER as usize) };
        let slot_ptr = unsafe { page_dir.add(slot_id as usize) };
        return slot_ptr;
    }

    pub fn page_create(&self, index: &Index, buf: &BufBlock, mtr: Mtr) {
        let ptr = buf.getFrame();

        /* CREATE THE INFIMUM AND SUPREMUM RECORDS */

        // Create tuple for infimum record
        {
            let dTuple = DTuple::new(1);
            dTuple.dtuple_set_info_bits(REC_STATUS_INFIMUM);

            let field = dTuple.dtuple_get_nth_field(0);
            field.dfield_set_data(
                "infimum", 8, /* size of infimum string*/
            );
            field.set_type(DATA_VARCHAR, DATA_ENGLIGH | DATA_NOT_NULL, 8);

            let heap_top =
                unsafe { self.ptr.add(PAGE_HEADER + PAGE_HEAP_TOP) };
            let infimum_rec = Record::new(heap_top)
                .rec_convert_dtuple_to_rec(index, dTuple, 0);
            infimum_rec.rec_set_n_owned(1);
            infimum_rec.rec_set_heap_no(0);
        }

        // Create tuple for supremum record
        {
            let dTuple = DTuple::new(1);
            dTuple.dtuple_set_info_bits(REC_STATUS_SUPREMUM);

            let field = dTuple.dtuple_get_nth_field(0);
            field.dfield_set_data(
                "supremum", 9, /* size of supremum string*/
            );
            field.set_type(DATA_VARCHAR, DATA_ENGLIGH | DATA_NOT_NULL, 8);

            let heap_top =
                unsafe { self.ptr.add(PAGE_HEADER + PAGE_HEAP_TOP) };
            let supremum_rec = Record::new(heap_top)
                .rec_convert_dtuple_to_rec(index, dTuple, 0);
            supremum_rec.rec_set_n_owned(1);
            supremum_rec.rec_set_heap_no(1);
        }

        /* 4. INITIALIZE THE PAGE */
        self.page_set_header_field(PAGE_N_DIR_SLOTS, 2);
        self.page_set_header_field(PAGE_HEAP_TOP, PAGE_SUPREMUM_END);
        self.page_set_header_field(PAGE_N_HEAP, PAGE_HEAP_NO_USER_LOW);
        self.page_set_header_field(PAGE_FREE, u16::MAX as usize);
        self.page_set_header_field(PAGE_GARBAGE, 0);
        self.page_set_header_field(PAGE_LAST_INSERT, u16::MAX as usize);
        self.page_set_header_field(PAGE_DIRECTION, PAGE_NO_DIRECTION);
        self.page_set_header_field(PAGE_N_DIRECTION, 0);
        self.page_set_header_field(PAGE_N_RECS, 0);
        self.set_max_trx_id(0, None);
        // Do I need to perform memset here?

        /* 5. SET POINTERS IN RECORDS AND DIR SLOTS */
    }

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
            rec = rec.rec_get_next();
        }
    }
}
