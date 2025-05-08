use crate::data::data0types::DTuple;
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
use std::ptr;

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
║ -1            ║ [0..7] → Next record pointer (low byte)      ║
║ -2            ║ [0..7] → Next record pointer (high byte)     ║
║ -3            ║ [7]    → Short flag                           ║
║               ║ [0..6] → Number of fields (bits 0–6)          ║
║ -4            ║ [6..7] → Number of fields (bits 7–9)          ║
║               ║ [0..5] → Heap number (bits 0–4)              ║
║ -5            ║ [0..7] → Heap number (bits 5–12)             ║
║ -6            ║ [4..7] → Number of records owned (n_owned)   ║
║               ║ [0..3] → Info bits (e.g., delete mark, etc.) ║
╚═══════════════╩══════════════════════════════════════════════╝
Read the table upwards.

[ info_bits (4) | n_owned (4) | heap_no (13) | n_fields (10) | short_flag (1) | next_rec_ptr (16) ]
*/

pub const REC_NEXT: usize = 2;
pub const REC_NEXT_MASK: usize = 0xFFFF;
pub const REC_NEXT_SHIFT: usize = 0;

pub const REC_SHORT: usize = 3;
pub const REC_SHORT_MASK: usize = 0x1;
pub const REC_SHORT_SHIFT: usize = 0;

pub const REC_N_FIELDS: usize = 4;
pub const REC_N_FIELDS_MASK: usize = 0x7FE;
pub const REC_N_FIELDS_SHIFT: usize = 0;

pub const REC_N_OWNED: usize = 6;
pub const REC_N_OWNED_MASK: usize = 0xF;
pub const REC_N_OWNED_SHIFT: usize = 0;

pub const REC_HEAP_NO: usize = 5;
pub const REC_HEAP_NO_MASK: usize = 0xFFF8;
pub const REC_HEAP_NO_SHIFT: usize = 0;

pub const REC_INFO_BITS: usize = 6;
pub const REC_INFO_BITS_MASK: usize = 0xF0;
pub const REC_INFO_BITS_SHIFT: usize = 4;

pub struct Record {
    ptr: *mut u8,
}

impl Record {
    pub fn new(ptr: *mut u8) -> Self {
        Record { ptr }
    }

    pub fn get_ptr(&self) -> *const u8 {
        self.ptr
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

    /// Get the next record chained to the current record.
    ///
    /// # Returns
    ///
    /// * A new Record object representing the next record.
    pub fn rec_get_next(&self) -> Record {
        unsafe {
            let offset = self.ptr.sub(REC_NEXT) as *const u16;
            let page_ptr = self.page_align();
            let next_ptr = page_ptr.add(*offset as usize);
            Record::new(next_ptr as *mut u8)
        }
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

    /// Gets the number of fields in the current record.
    ///
    /// # Returns
    ///
    /// * The number of fields in the current record.
    ///
    pub fn rec_get_n_fields(&self) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_N_FIELDS) as *const u16;
            println!("p: {:?}", *p);
            (*p as usize & REC_N_FIELDS_MASK) >> REC_N_FIELDS_SHIFT
        }
    }

    /// Sets the number of fields in the current record.
    ///
    /// # Arguments
    ///
    /// * `n_fields`: The number of fields to set.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn rec_set_n_fields(&self, n_fields: usize) {
        unsafe {
            let p = self.ptr.sub(REC_N_FIELDS) as *mut u16;
            let val = (*p as usize & !REC_N_FIELDS_MASK)
                | (n_fields << REC_N_FIELDS_SHIFT);
            p.write_unaligned(val as u16);
            println!("val: {:?}", *p);
        }
    }

    /// Gets the number of records owned by the current directory record.
    ///
    /// # Returns
    ///
    /// * The number of records owned by the current directory record.
    ///
    pub fn rec_get_n_owned(&self) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_N_OWNED);
            (*p as usize & REC_N_OWNED_MASK) >> REC_N_OWNED_SHIFT
        }
    }

    /// Get the info bits of the current record.
    ///
    /// # Returns
    ///
    /// * The info bits of the current record.
    ///     
    pub fn rec_get_info_bits(&self) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_INFO_BITS).read_unaligned();
            (p as usize & REC_INFO_BITS_MASK) >> REC_INFO_BITS_SHIFT
        }
    }

    /// Sets the info bits of the current record.
    ///
    /// # Arguments
    ///
    /// * `info_bits`: The info bits to set.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    ///
    pub fn rec_set_info_bits(&self, info_bits: usize) {
        unsafe {
            let p = self.ptr.sub(REC_INFO_BITS);
            let prev_val = p.read_unaligned();
            let val = (prev_val as usize & !REC_INFO_BITS_MASK)
                | (info_bits << REC_INFO_BITS_SHIFT);
            p.write_unaligned(val as u8);
        }
    }

    pub fn rec_set_n_owned(&self, n_owned: usize) {
        unimplemented!()
    }

    /// Sets the heap number of the current record.
    ///
    /// # Arguments
    ///
    /// * `heap_no`: The heap number to set.
    ///
    /// # Returns
    ///     
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn rec_set_heap_no(&self, heap_no: usize) {
        unsafe {
            let p = self.ptr.sub(REC_HEAP_NO) as *mut u16;
            let val = (*p as usize & !REC_HEAP_NO_MASK)
                | (heap_no << REC_HEAP_NO_SHIFT);
            *p = (val & REC_HEAP_NO_MASK) as u16;
        }
    }

    /// Gets the heap number of the current record.
    ///
    /// # Returns
    ///
    /// * The heap number of the current record.
    ///
    pub fn rec_get_heap_no(&self) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_HEAP_NO);
            (*p as usize & REC_HEAP_NO_MASK) >> REC_HEAP_NO_SHIFT
        }
    }

    /// Sets the offset of the next record in a physical record.
    ///
    /// # Arguments
    ///
    /// * `next`: The offset of the next record.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn rec_set_next_offs(&self, next: usize) {
        unsafe {
            let next_ptr = self.ptr.sub(REC_NEXT);
            ptr::write_unaligned(next_ptr as *mut u16, next as u16)
        }
    }

    /// Gets the offset of the next record in a physical record.
    ///
    /// # Returns
    ///
    /// * The offset of the next record.
    ///
    pub fn rec_get_next_offs(&self) -> usize {
        unsafe {
            let next_ptr = self.ptr.sub(REC_NEXT) as *const u16;
            next_ptr.read_unaligned() as usize
        }
    }
}
