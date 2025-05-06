use crate::data::data0types::DTuple;
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
use std::ptr::null_mut;

/** Number of extra bytes in a record in addition to the data and offsets. */
/** Record status values */
pub const REC_STATUS_ORDINARY: usize = 0;
pub const REC_STATUS_NODE_PTR: usize = 1;
pub const REC_STATUS_INFIMUM: usize = 2;
pub const REC_STATUS_SUPREMUM: usize = 3;

pub const REC_N_EXTRA_BYTES: usize = 6;
/***
The record header is 6 bytes long and contains the following fields:

1. Next record pointer (2 bytes)
2. Short flag (1 bit)
3. Number of fields (7 bits)
4. Heap number (5 bits)
5. Number of records owned (4 bits)
6. Info bits (4 bits)

Row Format: [... record header | data]

The layout of the record header is as follows:
╔══════════════╦══════════════════════════════════════════════╗
║ Byte Offset  ║ Bit Usage (within byte)                      ║
╠══════════════╬══════════════════════════════════════════════╣
║ 1            ║ [0..7] → Next record pointer (low byte)      ║
║ 2            ║ [0..7] → Next record pointer (high byte)     ║
║ 3            ║ [0]    → Short flag                           ║
║              ║ [1..7] → Number of fields (bits 0–6)          ║
║ 4            ║ [0..2] → Number of fields (bits 7–9)          ║
║              ║ [3..7] → Heap number (bits 0–4)              ║
║ 5            ║ [0..7] → Heap number (bits 5–12)             ║
║ 6            ║ [0..3] → Number of records owned (n_owned)   ║
║              ║ [4..7] → Info bits (e.g., delete mark, etc.) ║
╚══════════════╩══════════════════════════════════════════════╝

[ info_bits (4) | n_owned (4) | heap_no (13) | n_fields (10) | short_flag (1) | next_rec_ptr (16) ]
*/

pub const REC_N_OWNED: usize = 6;
pub const REC_N_OWNED_MASK: usize = 0xF;
pub const REC_N_OWNED_SHIFT: usize = 0;

pub struct Record {
    ptr: *mut u8,
}

impl Record {
    pub fn new(ptr: *mut u8) -> Self {
        Record { ptr }
    }

    pub fn get_ptr(&self) -> *const u8 {
        return self.ptr;
    }

    /// Aligns the record pointer to the specified offset.
    ///
    /// # Arguments
    ///
    /// * `offset`: The offset to align to.
    ///
    /// returns: *const u8
    fn align_offset(&self, offset: usize) -> *const u8 {
        unsafe { self.ptr.add(offset) }
    }

    pub fn page_align(&self) -> *const u8 {
        let align = UNIV_PAGE_SIZE;
        let addr = self.ptr as usize;
        let aligned_addr = addr & !(align - 1); // Align down by masking lower bits
        aligned_addr as *const u8
    }

    pub fn rec_get_next_ptr(&self) -> *const u8 {
        let next_ptr_offset = 0; // Replace with the actual offset for the next pointer
        self.align_offset(next_ptr_offset)
    }

    pub fn rec_get_next(&self) -> Record {
        Record::new(null_mut())
    }

    /// Builds a physical record out of a data tuple, and stores it in the record.
    ///
    /// # Arguments
    ///
    /// * `index`: The index to which the record belongs.
    /// * `dTuple`: The data tuple to convert.
    /// * `n_ext`: The number of external fields.
    ///
    pub fn rec_convert_dtuple_to_rec(
        &self,
        index: &Index,
        dTuple: DTuple,
        n_ext: usize,
    ) -> Record {
        unimplemented!()
    }

    pub fn rec_get_n_owned(&self) -> usize {
        unsafe {
            let p = self.ptr.add(REC_N_OWNED);
            (*p as usize & REC_N_OWNED_MASK) >> REC_N_OWNED_SHIFT
        }
    }

    pub fn rec_set_n_owned(&self, n_owned: usize) {
        unimplemented!()
    }

    pub fn rec_set_heap_no(&self, heap_no: usize) {
        unimplemented!()
    }

    pub fn rec_get_heap_no(&self) -> usize {
        let heap_no_offset = 0; // Replace with the actual offset for the heap number
        let heap_no_ptr = self.align_offset(heap_no_offset);
        unsafe { *(heap_no_ptr as *const usize) }
    }
}
