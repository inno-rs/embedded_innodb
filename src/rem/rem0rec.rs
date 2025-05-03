use crate::data::data0types::DTuple;
use crate::dict::dict0types::Index;
use std::ptr::null_mut;

/** Number of extra bytes in a record in addition to the data and offsets. */
/** Record status values */
pub const REC_STATUS_ORDINARY: usize = 0;
pub const REC_STATUS_NODE_PTR: usize = 1;
pub const REC_STATUS_INFIMUM: usize = 2;
pub const REC_STATUS_SUPREMUM: usize = 3;

pub const REC_N_EXTRA_BYTES: usize = 6;
pub struct Record {
    ptr: *mut u8,
}

impl Record {
    pub fn new(ptr: *mut u8) -> Self {
        Record { ptr }
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
        let align = 16 * 1024;
        let addr = self.ptr as usize;
        let aligned_addr = addr & !(align - 1); // Align down by masking lower bits
        return aligned_addr as *const u8;
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
