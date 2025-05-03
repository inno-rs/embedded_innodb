/** Number of extra bytes in a record in addition to the data and offsets. */
pub const REC_N_EXTRA_BYTES: usize = 6;
pub struct Record {
    ptr: *const u8,
}

impl Record {
    pub fn new(ptr: *const u8) -> Self {
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
        let next_offset = 0; // Replace with the actual offset for the next record
        Record::new(self.align_offset(next_offset))
    }

    pub fn rec_get_heap_no(&self) -> usize {
        let heap_no_offset = 0; // Replace with the actual offset for the heap number
        let heap_no_ptr = self.align_offset(heap_no_offset);
        unsafe { *(heap_no_ptr as *const usize) }
    }
}
