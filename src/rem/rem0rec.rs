use std::{fmt, ptr, slice};

use tracing::{debug, error};

use crate::{
    data::data0types::DTuple,
    dict::dict0types::Index,
    innodb::innodb0types::{UNIV_PAGE_SIZE, UNIV_SQL_NULL},
    mach::mach0data::{
        mach_read_from_1, mach_read_from_2, mach_write_to_1, mach_write_to_2,
    },
    page::page0page::Page,
    rem::rem0types::REC_MAX_N_FIELDS,
    ut::{ut0dbg::ut_ad, ut0math::ut_align_down},
    ut_panic,
};

/// Info bit denoting the predefined minimum record:
/// this bit is set if and only if the record
/// is the first user record on a non-leaf B-Tree page that is the leftmost page on its level.
pub const REC_INFO_MIN_REC_FLAG: usize = 0x10;

/** Number of extra bytes in a record in addition to the data and offsets. */
/** Record status values */
pub const REC_STATUS_ORDINARY: usize = 0;
/// Non-leaf record.
/// The first n fields are from record, and the last one is the pointer to child node page no.
pub const REC_STATUS_NODE_PTR: usize = 1;
pub const REC_STATUS_INFIMUM: usize = 2;
pub const REC_STATUS_SUPREMUM: usize = 3;

pub const REC_N_EXTRA_BYTES: usize = 6;

/// Maximum length of the data in the physical record if the offsets are in 1-byte format.
pub const REC_1BYTE_OFFS_LIMIT: usize = 0x7F;

/// The record header is 6 bytes long and contains the following fields:
///
/// 1. Next record pointer (2 bytes)
/// 2. Short flag (1 bit)
/// 3. Number of fields (7 bits)
/// 4. Heap number (5 bits)
/// 5. Number of records owned (4 bits)
/// 6. Info bits (4 bits)
///
/// Row Format: [... record header | data]
///
/// The layout of the record header is as follows:
/// ```text
/// ╔══════════════╦══════════════════════════════════════════════╗
/// ║ Byte Offset  ║ Bit Usage (within byte)                      ║
/// ╠══════════════╬══════════════════════════════════════════════╣
/// ║ -1           ║ [0..7] → Next record pointer (low byte)      ║
/// ║ -2           ║ [0..7] → Next record pointer (high byte)     ║
/// ║ -3           ║ [7]    → Short flag                          ║
/// ║              ║ [0..6] → Number of fields (bits 0–6)         ║
/// ║ -4           ║ [6..7] → Number of fields (bits 6–7)         ║
/// ║              ║ [0..5] → Heap number (bits 0–5)              ║
/// ║ -5           ║ [0..7] → Heap number (bits 5–12)             ║
/// ║ -6           ║ [4..7] → Number of records owned (n_owned)   ║
/// ║              ║ [0..3] → Info bits (e.g., delete mark, etc.) ║
/// ╚══════════════╩══════════════════════════════════════════════╝
/// ```
/// Read the table upwards.
///
/// [ info_bits (4) | n_owned (4) | heap_no (13) | n_fields (10) | short_flag (1) | next_rec_ptr (16) ]

pub const REC_NEXT: usize = 2;
pub const REC_NEXT_MASK: usize = 0xFFFF;
pub const REC_NEXT_SHIFT: usize = 0;

pub const REC_SHORT: usize = 3;
pub const REC_SHORT_MASK: usize = 0x1;
pub const REC_SHORT_SHIFT: usize = 0;

pub const REC_N_FIELDS: usize = 4;
pub const REC_N_FIELDS_MASK: usize = 0x7FE;
pub const REC_N_FIELDS_SHIFT: usize = 1;

pub const REC_N_OWNED: usize = 6;
pub const REC_N_OWNED_MASK: usize = 0xF;
pub const REC_N_OWNED_SHIFT: usize = 0;

pub const REC_HEAP_NO: usize = 5;
pub const REC_HEAP_NO_MASK: usize = 0xFFF8;
pub const REC_HEAP_NO_SHIFT: usize = 3;

pub const REC_INFO_BITS: usize = 6;
pub const REC_INFO_BITS_MASK: usize = 0xF0;
pub const REC_INFO_BITS_SHIFT: usize = 4;

/// The following masks are used to filter the SQL null bit from the one byte and two byte offsets.
pub const REC_1BYTE_SQL_NULL_MASK: usize = 0x80;
pub const REC_2BYTE_SQL_NULL_MASK: usize = 0x8000;

/// Bit to specify that the column is stored externally.
/// 1 byte format (aka REC_SHORT) does not support external fields.
/// We have to use 2 byte format if we want to store fields externally.
pub const REC_OFFS_EXTERNAL: u16 = 0x4000;
pub const REC_OFFS_EXTERNAL_MASK: u16 = 0x3FFF;

/// Record data enum.
#[derive(Debug)]
pub enum RecordFieldData {
    Data(String),
    SqlNullData(),
}

impl RecordFieldData {
    /// Check if the data is SQL null.
    ///
    /// # Returns
    ///
    /// A bool specifying if the record field data is null or not.
    pub fn is_sql_null(&self) -> bool {
        match self {
            RecordFieldData::Data(_) => false,
            RecordFieldData::SqlNullData() => true,
        }
    }

    /// Get data from RecordData or panic if SQL null.
    ///
    /// # Returns
    ///
    /// String representation of the record data.
    /// Panic otherwise.
    pub fn data_or_panic(&self) -> String {
        match self {
            RecordFieldData::Data(d) => d.to_string(),
            RecordFieldData::SqlNullData() => {
                ut_panic!("field has data and is not null")
            }
        }
    }
}

/// Sometimes we need records that don't have a backing page.
/// In such cases, we can create a block of memory with the appropriate size using the n_fields,
/// and total size information.
/// When the OrphanRecord is destroyed, the backing memory is also destroyed.
pub struct OrphanRecord {
    data: Vec<u8>,
    n_fields: usize,
}

impl OrphanRecord {
    /// Creates a new OrphanRecord with the provided data and number of fields.
    ///
    /// # Arguments
    ///
    /// * `data` - A vector of bytes that will serve as the backing memory for the record.
    /// * `n_fields` - The number of fields in the record.
    ///
    /// # Returns
    ///
    /// A new OrphanRecord instance.
    pub fn new(data: Vec<u8>, n_fields: usize) -> Self {
        OrphanRecord { data, n_fields }
    }

    /// Returns a Record instance that points to the appropriate location within the backing memory.
    ///
    /// This method calculates the correct offset in the backing memory
    /// where the record data should start,
    /// taking into account the extra bytes needed for record metadata and field offsets.
    ///
    /// # Returns
    ///
    /// A Record instance pointing to the record data within the backing memory.
    pub fn get_record(&mut self) -> Record {
        unsafe {
            Record::new(
                self.data.as_mut_ptr().add(REC_N_EXTRA_BYTES + self.n_fields),
            )
        }
    }
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub struct Record {
    // todo: Move to slice.
    /// Pointer to the beginning of the record.
    ptr: *mut u8,
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n_fields = self.rec_get_n_fields();
        let info_bits = self.rec_get_info_bits();
        let byte_format = if self.rec_get_1byte_offs_flag() { 1 } else { 2 };

        write!(
            f,
            "PHYSICAL RECORD: n_fields {}; {}-byte offsets; info bits {}; n owned {};",
            n_fields,
            byte_format,
            info_bits,
            self.rec_get_n_owned()
        )?;

        write!(f, " fields ")?;

        for n_field in 0..n_fields {
            write!(f, " {}: ", n_field)?;

            let data = self.rec_get_data(n_field);
            match data {
                None => {
                    ut_panic!("rec_to_string: data out of range");
                }
                Some(d) => match d {
                    RecordFieldData::Data(data) => {
                        let len = data.len();
                        write!(f, " {}; len: {} ", data, len)?;
                    }
                    RecordFieldData::SqlNullData() => {
                        write!(f, " SQL NULL, len: {}", 5)?;
                    }
                },
            }
        }

        Ok(())
    }
}

impl Record {
    /// Creates a new Record instance with the given pointer.
    ///
    /// # Arguments
    ///
    /// * `ptr` - A pointer to the beginning of the record data.
    ///
    /// # Returns
    ///
    /// * A new Record instance.
    pub fn new(ptr: *mut u8) -> Self {
        Record { ptr }
    }

    /// Returns the pointer to the beginning of the record data.
    ///
    /// # Returns
    ///
    /// * A pointer to the beginning of the record data.
    pub fn get_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Copies the first n fields of the physical record to a new physical record.
    ///
    /// # Arguments
    ///
    /// `index`: The index associated with the record.
    /// `n_fields`: Number of fields to copy.
    ///
    /// # Return
    ///
    /// Vector containing the copied record.
    pub fn rec_copy_prefix_to_buf(
        &self,
        index: &Index,
        n_fields: usize,
    ) -> Vec<u8> {
        let area_start = if self.rec_get_1byte_offs_flag() {
            REC_N_EXTRA_BYTES + n_fields
        } else {
            REC_N_EXTRA_BYTES + 2 * n_fields
        };
        let area_end = self.rec_get_field_start_offs(n_fields);

        let prefix_length = area_start + area_end;

        let mut buf: Vec<u8> = vec![0; prefix_length];

        unsafe {
            buf.as_mut_ptr()
                .copy_from(self.ptr.sub(area_start), prefix_length);
        }

        let rec = Record::new(buf.as_mut_ptr());
        rec.rec_set_n_fields(n_fields);

        buf
    }

    /// Copies the content of this record to another record.
    ///
    /// # Arguments
    ///
    /// * `copy_to` - The destination record to copy to.
    ///
    /// # Returns
    ///
    /// * `()` - This function does not return a value.
    pub fn rec_copy(&self, copy_to: &mut Record) {
        let data_size = self.rec_get_data_size();
        let extra_len = self.rec_get_extra_size();

        unsafe {
            copy_to
                .ptr
                .sub(extra_len)
                .copy_from(self.ptr.sub(extra_len), data_size + extra_len);
        }
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

    /// Aligns the record pointer to the page size.
    ///
    /// # Returns
    ///
    /// *const u8
    pub fn page_align(&self) -> *mut u8 {
        let addr = self.ptr as usize;
        let aligned_addr = ut_align_down(addr, UNIV_PAGE_SIZE); // Align down by masking lower bits
        aligned_addr as *mut u8
    }

    /// Returns the page that contains this record.
    ///
    /// # Returns
    ///
    /// * A Page instance representing the page that contains this record.
    pub fn get_page(&self) -> Page {
        Page::new(self.page_align())
    }

    /// Get the next record chained to the current record.
    ///
    /// # Returns
    ///
    /// * A new Record object representing the next record.
    pub fn rec_get_next(&self) -> Option<Record> {
        unsafe {
            let offset = self.rec_get_next_offs();
            if offset == 0 {
                // There is no next record
                return None;
            }

            ut_ad!(offset < UNIV_PAGE_SIZE, "offset={}", offset);

            let page_ptr = self.page_align();
            let next_ptr = page_ptr.add(offset);
            Some(Record::new(next_ptr))
        }
    }

    /// Get the previous record chained to the current record.
    ///
    /// # Returns
    ///
    /// * A new Record object representing the previous record.
    ///
    pub fn rec_get_prev(&self) -> Option<Record> {
        let page = Page::new(self.page_align());

        let slot_no = page.page_dir_find_owner_slot(self.clone());
        let mut rec = page.page_dir_slot_get_rec(slot_no - 1);

        let mut prev_rec = rec;
        while rec.get_ptr() != self.ptr {
            prev_rec = rec;

            match rec.rec_get_next() {
                Some(next_rec) => rec = next_rec,
                None => ut_panic!("rec_get_prev: no next record"),
            }
        }

        Some(prev_rec)
    }

    /// Builds a physical record out of a data tuple and stores it in the record.
    ///
    /// # Arguments
    ///
    /// * `index`: The index to which the record belongs.
    /// * `d_tuple`: The data tuple to convert.
    /// * `n_ext`: The number of external fields.
    ///
    /// # Returns
    ///
    /// * A new Record instance containing the converted data tuple.
    ///
    pub fn rec_convert_dtuple_to_rec(
        &self,
        index: &Index,
        dtuple: &DTuple,
        n_ext: usize,
    ) -> Record {
        ut_ad!(dtuple.dtuple_validate());
        ut_ad!(dtuple.dtuple_type_checked());

        let ret = Record { ptr: self.ptr };

        let n_fields = dtuple.dtuple_get_n_fields();
        let data_size = dtuple.dtuple_get_data_size();

        /* Calculate the offset of the origin in the physical record */

        /* Store the number of fields */
        self.rec_set_n_fields(n_fields);

        /* Store the info bits of the record */
        self.rec_set_info_bits(dtuple.dtuple_get_info_bits());

        /* Store the data and offsets */
        let mut end_offset = 0;

        if n_ext == 0 && data_size < REC_1BYTE_OFFS_LIMIT {
            self.rec_set_1byte_offs_flag(true);

            for i in 0..n_fields {
                let field = dtuple.dtuple_get_nth_field(i);
                let ored_offset: usize;

                match field.dfield_is_null() {
                    true => {
                        let dtype = field.dfield_get_type();
                        let length = dtype.dtype_get_sql_null_size();

                        if length != 0 {
                            // Write an SQL null field full of zeros.
                            unsafe {
                                let ptr = self.ptr.add(end_offset);
                                ptr::write_bytes(ptr, 0, length);
                            }
                        }

                        end_offset += length;

                        ored_offset = end_offset | REC_1BYTE_SQL_NULL_MASK;
                    }
                    false => {
                        /* If the data is not an SQL nullptr, store it. */
                        let length = field.dfield_get_len();
                        unsafe {
                            let ptr = self.ptr.add(end_offset);
                            let src = field.dfield_get_data().as_ptr();
                            ptr.copy_from_nonoverlapping(src, length);
                        }

                        end_offset += length;
                        ored_offset = end_offset;
                    }
                }

                self.rec_1_set_field_end_info(i, ored_offset);
                ut_ad!(ret.rec_1_get_field_end_info(i) == ored_offset)
            }
        } else {
            self.rec_set_1byte_offs_flag(false);

            for i in 0..n_fields {
                let field = dtuple.dtuple_get_nth_field(i);
                let ored_offset: usize;

                match field.dfield_is_null() {
                    true => {
                        let dtype = field.dfield_get_type();
                        let length = dtype.dtype_get_sql_null_size();

                        if length != 0 {
                            // Write an SQL null field full of zeros.
                            unsafe {
                                let ptr = self.ptr.add(end_offset);
                                ptr::write_bytes(ptr, 0, length);
                            }
                        }

                        end_offset += length;

                        ored_offset = end_offset | REC_2BYTE_SQL_NULL_MASK;
                    }
                    false => {
                        /* If the data is not an SQL nullptr, store it. */
                        let length = field.dfield_get_len();
                        unsafe {
                            let ptr = self.ptr.add(end_offset);
                            let src = field.dfield_get_data().as_ptr();
                            ptr.copy_from_nonoverlapping(src, length);
                        }

                        end_offset += length;
                        ored_offset = end_offset;
                    }
                }

                self.rec_2_set_field_end_info(i, ored_offset);
                ut_ad!(ret.rec_2_get_field_end_info(i) == ored_offset)
            }
        }

        ret
    }

    /// Gets the number of fields in the current record.
    ///
    /// # Returns
    ///
    /// * The number of fields in the current record.
    ///
    pub fn rec_get_n_fields(&self) -> usize {
        self.rec_get_bit_field_2(
            REC_N_FIELDS,
            REC_N_FIELDS_MASK,
            REC_N_FIELDS_SHIFT,
        )
    }

    /// Sets the number of fields in the current record.
    ///
    /// # Arguments
    ///
    /// * `n_fields`: The number of fields to set.
    pub fn rec_set_n_fields(&self, n_fields: usize) {
        debug!(
            "rec_set_n_fields: n_fields={} at={}",
            n_fields, self.ptr as usize
        );

        self.rec_set_bit_field_2(
            n_fields,
            REC_N_FIELDS,
            REC_N_FIELDS_MASK,
            REC_N_FIELDS_SHIFT,
        );
    }

    /// Gets the number of records owned by the current directory record.
    ///
    /// # Returns
    ///
    /// * The number of records owned by the current directory record.
    ///
    pub fn rec_get_n_owned(&self) -> usize {
        self.rec_get_bit_field_1(
            REC_N_OWNED,
            REC_N_OWNED_MASK,
            REC_N_OWNED_SHIFT,
        )
    }

    /// Get the info bits of the current record.
    ///
    /// # Returns
    ///
    /// * The info bits of the current record.
    ///     
    pub fn rec_get_info_bits(&self) -> usize {
        self.rec_get_bit_field_1(
            REC_INFO_BITS,
            REC_INFO_BITS_MASK,
            REC_INFO_BITS_SHIFT,
        )
    }

    /// Sets the info bits of the current record.
    ///
    /// # Arguments
    ///
    /// * `info_bits`: The info bits to set.
    pub fn rec_set_info_bits(&self, info_bits: usize) {
        self.rec_set_bit_field_1(
            info_bits,
            REC_INFO_BITS,
            REC_INFO_BITS_MASK,
            REC_INFO_BITS_SHIFT,
        )
    }

    /// Sets the number of records owned by the current directory record.
    ///
    /// # Arguments
    ///
    /// * `n_owned`: The number of records owned to set.
    pub fn rec_set_n_owned(&self, n_owned: usize) {
        self.rec_set_bit_field_1(
            n_owned,
            REC_N_OWNED,
            REC_N_OWNED_MASK,
            REC_N_OWNED_SHIFT,
        );
    }

    /// Sets the heap number of the current record.
    ///
    /// # Arguments
    ///
    /// * `heap_no`: The heap number to set.
    pub fn rec_set_heap_no(&self, heap_no: usize) {
        self.rec_set_bit_field_2(
            heap_no,
            REC_HEAP_NO,
            REC_HEAP_NO_MASK,
            REC_HEAP_NO_SHIFT,
        );
    }

    /// Gets the heap number of the current record.
    ///
    /// # Returns
    ///
    /// * The heap number of the current record.
    ///
    pub fn rec_get_heap_no(&self) -> usize {
        self.rec_get_bit_field_2(
            REC_HEAP_NO,
            REC_HEAP_NO_MASK,
            REC_HEAP_NO_SHIFT,
        )
    }

    /// Sets the offset of the next record in a physical record.
    ///
    /// # Arguments
    ///
    /// * `next`: The offset of the next record.
    pub fn rec_set_next_offs(&self, next: usize) {
        self.rec_set_bit_field_2(
            next,
            REC_NEXT,
            REC_NEXT_MASK,
            REC_NEXT_SHIFT,
        );
    }

    /// Gets the offset of the next record in a physical record.
    ///
    /// # Returns
    ///
    /// * The offset of the next record.
    ///
    pub fn rec_get_next_offs(&self) -> usize {
        self.rec_get_bit_field_2(REC_NEXT, REC_NEXT_MASK, REC_NEXT_SHIFT)
    }

    /// Reads the start offset of the data field in the record.
    ///
    /// The start of the SQL null field is the end offset of the previous non-null field,
    /// or 0. If n is the number of last field + 1,
    /// then the end offset of the last field is returned.
    ///
    /// # Arguments
    ///
    /// * `field_no` - The field number to get the start offset for.
    ///
    /// # Returns
    ///
    /// * The start offset of the specified field in the record.
    pub fn rec_get_field_start_offs(&self, field_no: usize) -> usize {
        if field_no == 0 {
            0
        } else if self.rec_get_1byte_offs_flag() {
            self.rec_1_get_field_start_offs(field_no)
        } else {
            self.rec_2_get_field_start_offs(field_no)
        }
    }

    /// Sets the field end for the nth field if the record is stored in the 2-byte format.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to set.
    /// * `field_end`: The field end offset to set.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn rec_2_set_field_end_info(&self, field_no: usize, field_end: usize) {
        unsafe {
            let p =
                self.ptr.sub(REC_N_EXTRA_BYTES + 2 * field_no + 2) as *mut u16;
            ptr::write_unaligned(p, field_end as u16)
        }
    }

    /// Returns the offset of the nth field end if the record is stored in a 2-byte format.
    /// If the field is SQL nullptr, the flag is ORed in the returned value.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the nth field end in the physical record.
    pub fn rec_2_get_field_end_info(&self, field_no: usize) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_N_EXTRA_BYTES + 2 * field_no + 2)
                as *const u16;
            p.read_unaligned() as usize
        }
    }

    /// Sets the field end for the nth field if the record is stored in the 1-byte format.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to set.
    /// * `field_end`: The field end offset to set.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn rec_1_set_field_end_info(&self, field_no: usize, field_end: usize) {
        unsafe {
            let p = self.ptr.sub(REC_N_EXTRA_BYTES + field_no + 1);
            ptr::write_unaligned(p, field_end as u8)
        }
    }

    /// Returns the offset of the (n-1)th field end in the physical record in the 1-byte offset format.
    /// If the field is SQL nullptr, the flag is ORed in the returned value.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the (n-1)th field end in the physical record.
    pub fn rec_1_get_prev_field_end_info(&self, field_no: usize) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_N_EXTRA_BYTES + field_no);
            p.read_unaligned() as usize
        }
    }

    /// Returns the offset of the (n-1)th field end in the physical record in the 2-byte offset format.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the (n-1)th field end in the physical record.
    ///
    pub fn rec_2_get_prev_field_end_info(&self, field_no: usize) -> usize {
        unsafe {
            let p =
                self.ptr.sub(REC_N_EXTRA_BYTES + field_no * 2) as *const u16;
            p.read_unaligned() as usize
        }
    }

    /// Gets the offset of the nth field start in the physical record in the 1-Byte offset form.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the nth field starts in the physical record.
    pub fn rec_1_get_field_start_offs(&self, field_no: usize) -> usize {
        if field_no == 0 {
            return 0;
        }

        self.rec_1_get_prev_field_end_info(field_no) & !REC_1BYTE_SQL_NULL_MASK
    }

    /// Gets the offset of the nth field start in the physical record in the 2-Byte offset form.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the nth field starts in the physical record.
    ///
    pub fn rec_2_get_field_start_offs(&self, field_no: usize) -> usize {
        if field_no == 0 {
            return 0;
        }

        self.rec_2_get_prev_field_end_info(field_no) & !REC_2BYTE_SQL_NULL_MASK
    }

    /// Returns the offset of the nth field end if the record is stored in a 1-byte format.
    /// If the field is SQL nullptr, the flag is ORed in the returned value.
    ///
    /// # Arguments
    ///
    /// * `field_no`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the nth field end in the physical record.
    pub fn rec_1_get_field_end_info(&self, field_no: usize) -> usize {
        unsafe {
            let p = self.ptr.sub(REC_N_EXTRA_BYTES + field_no + 1);
            p.read_unaligned() as usize
        }
    }

    /// Gets the offset of the nth data field in the physical record.
    ///
    /// # Arguments
    ///
    /// * `n_field`: The field number to get the offset for.
    ///
    /// # Returns
    ///
    /// * The offset of the nth data field in the physical record.
    /// * The length of the nth data field.
    pub fn rec_get_nth_field_offs(&self, n_field: usize) -> (usize, usize) {
        let fields_count = self.rec_get_n_fields();
        if n_field >= fields_count {
            ut_panic!(
                "rec_get_nth_field_offs: n_field out of range. fields_count={}",
                fields_count
            );
        }

        if self.rec_get_1byte_offs_flag() {
            let offset = self.rec_1_get_field_start_offs(n_field);
            let next_offset = self.rec_1_get_field_end_info(n_field);

            if next_offset & REC_1BYTE_SQL_NULL_MASK != 0 {
                return (offset, UNIV_SQL_NULL);
            }

            return (offset, next_offset - offset);
        }

        let offset = self.rec_2_get_field_start_offs(n_field);
        let next_offset = self.rec_2_get_field_end_info(n_field);

        if next_offset & REC_2BYTE_SQL_NULL_MASK != 0 {
            return (offset, UNIV_SQL_NULL);
        }

        (offset, next_offset - offset)
    }

    /// Gets the nth field from the record.
    ///
    /// # Arguments
    ///
    /// * `n_field` - The field number to retrieve.
    ///
    /// # Returns
    ///
    /// * A tuple containing the offset and size of the field.
    ///
    /// # Panics
    ///
    /// * If the field number is out of range.
    pub fn rec_get_nth_field(&self, n_field: usize) -> (usize, usize) {
        let fields_count = self.rec_get_n_fields();
        if n_field >= fields_count {
            ut_panic!("rec_get_nth_field: n_field out of range");
        }

        self.rec_get_nth_field_offs(n_field)
    }

    /// Returns the data size of a physical record which is the sum of field lengths.
    ///
    /// SQL null fields are counted as lengths of zero. The value returned by the function
    /// is the difference between record origin and record end.
    ///
    /// # Returns
    ///
    /// * The total data size of the record in bytes.
    pub fn rec_get_data_size(&self) -> usize {
        self.rec_get_field_start_offs(self.rec_get_n_fields())
    }

    /// Gets the data of the nth field in the physical record.
    ///
    /// # Arguments
    ///
    /// * `n_field`: The field number to get the data for.
    ///
    /// # Returns
    ///
    /// * The data of the nth field as a String.
    /// * `None` if the field number is out of range.
    pub fn rec_get_data(&self, n_field: usize) -> Option<RecordFieldData> {
        ut_ad!(self.rec_get_n_fields() > n_field);

        let (offset, length) = self.rec_get_nth_field_offs(n_field);
        debug!(
            "rec_get_data: ptr={} offset={}, length={}",
            self.ptr as usize, offset, length
        );

        if length == UNIV_SQL_NULL {
            return Some(RecordFieldData::SqlNullData());
        }

        unsafe {
            let bytes = slice::from_raw_parts(self.ptr.add(offset), length);
            let data = std::str::from_utf8_unchecked(bytes).to_string();
            Some(RecordFieldData::Data(data))
        }
    }

    /// Get the extra size of the record.
    ///
    /// # Returns
    ///
    /// Total extra size occupied by the record.
    pub fn rec_get_extra_size(&self) -> usize {
        let data_size = self.rec_get_data_size();
        let n_fields = self.rec_get_n_fields();
        Self::rec_get_converted_extra_size(data_size, n_fields, 0)
    }

    /// Returns the extra size of a physical record if we know its data size and number of fields.
    ///
    ///  # Arguments
    ///
    /// * `data_size`: Total size of all the data fields.
    /// * `n_fields`: Number of fields.
    /// * `n_ext` Number of external fields.
    ///
    /// # Return
    ///
    /// Total extra size of the record.
    pub fn rec_get_converted_extra_size(
        data_size: usize,
        n_fields: usize,
        n_ext: usize,
    ) -> usize {
        if n_ext == 0 && data_size <= REC_1BYTE_OFFS_LIMIT {
            return REC_N_EXTRA_BYTES + n_fields;
        }

        REC_N_EXTRA_BYTES + n_fields * 2
    }

    /// Returns the total size of the physical record in bytes.
    ///
    /// This includes both the data size and the extra bytes used for record metadata.
    ///
    /// # Returns
    ///
    /// * The total size of the record in bytes.
    pub fn rec_offs_size(&self) -> usize {
        // n_fields is the total number of fields.
        let n_fields = self.rec_get_n_fields();
        let (end_offset_n_field, n_fields) =
            match self.rec_get_1byte_offs_flag() {
                true => {
                    let mut end_offset_n_field =
                        self.rec_1_get_field_end_info(n_fields - 1);

                    if end_offset_n_field & REC_1BYTE_SQL_NULL_MASK
                        == REC_1BYTE_SQL_NULL_MASK
                    {
                        end_offset_n_field &= !REC_1BYTE_SQL_NULL_MASK;
                    }

                    (end_offset_n_field, n_fields)
                }
                false => {
                    let mut end_offset_n_field =
                        self.rec_2_get_field_end_info(n_fields - 1);

                    if end_offset_n_field & REC_2BYTE_SQL_NULL_MASK
                        == REC_2BYTE_SQL_NULL_MASK
                    {
                        end_offset_n_field &= !REC_2BYTE_SQL_NULL_MASK;
                    }

                    (end_offset_n_field, n_fields * 2)
                }
            };

        // The offset of the n-field end is measured from the record start.
        // Before the record starts,
        // we have metadata about the record.
        REC_N_EXTRA_BYTES + n_fields + end_offset_n_field
    }

    /// Check whether the field offsets are stored in 1 byte or 2 byte format.
    ///
    /// # Returns
    ///
    /// True, if the format is 1 byte.
    /// False, otherwise (2 byte).
    pub fn rec_get_1byte_offs_flag(&self) -> bool {
        self.rec_get_bit_field_1(REC_SHORT, REC_SHORT_MASK, REC_SHORT_SHIFT)
            == 1
    }

    /// Set the value for 1-byte offset flag.
    /// If it is not a 1-byte format, it is assumed to be a 2-byte format.
    ///
    /// # Arguments
    ///
    /// `flag`: A boolean specifying if it is a 1-byte format or not.
    ///
    pub fn rec_set_1byte_offs_flag(&self, flag: bool) {
        let val = if flag { 1 } else { 0 };
        self.rec_set_bit_field_1(
            val,
            REC_SHORT,
            REC_SHORT_MASK,
            REC_SHORT_SHIFT,
        );
    }

    /// Returns true if the extern bit is set on the nth field.
    ///
    /// # Arguments
    ///
    /// `n_field`: The nth field.
    ///
    /// # Returns
    ///
    /// True if the nth field is external.
    /// False, otherwise.
    pub fn rec_nth_external(&self, n_field: usize) -> bool {
        let (offset, _size) = self.rec_get_nth_field_offs(n_field);

        if self.rec_get_1byte_offs_flag() {
            false
        } else {
            offset as u16 & REC_OFFS_EXTERNAL_MASK == REC_OFFS_EXTERNAL
        }
    }

    /// Copies a prefix of the record to dtuple.
    ///
    /// # Arguments
    ///
    /// `n_fields`: The number of fields to copy.
    pub fn rec_copy_prefix_to_dtuple(
        &self,
        n_fields: usize,
        dtuple: &mut DTuple,
    ) {
        ut_ad!(self.rec_validate());

        dtuple.dtuple_set_info_bits(self.rec_get_info_bits());

        ut_ad!(dtuple.dtuple_type_checked());

        for field_index in 0..n_fields {
            let dfield = dtuple.dtuple_get_nth_field_mut(field_index);
            let rec_field = self.rec_get_data(field_index);

            ut_ad!(rec_field.is_some());

            let data = rec_field.unwrap();

            match data {
                RecordFieldData::Data(d) => {
                    dfield.dfield_set_data(d.as_bytes().to_vec(), d.len());
                    // handle extern field??
                }
                RecordFieldData::SqlNullData() => {
                    dfield.dfield_set_null();
                }
            }
        }
    }

    /// Gets the size of the nth field in the record.
    ///
    /// # Arguments
    ///
    /// * `n` - The field number to get the size for.
    ///
    /// # Returns
    ///
    /// * The size of the field in bytes.
    pub fn rec_get_nth_field_size(&self, n: usize) -> usize {
        let (offset, size) = self.rec_get_nth_field_offs(n);
        size
    }

    /// Validates the record structure.
    ///
    /// This method checks if the record has a valid number of fields and if the sum of field lengths
    /// matches the total data size.
    ///
    /// # Returns
    ///
    /// * `true` if the record is valid, `false` otherwise.
    pub fn rec_validate(&self) -> bool {
        let n_fields = self.rec_get_n_fields();

        if n_fields == 0 || n_fields >= REC_MAX_N_FIELDS {
            error!("record has {} fields", n_fields);
            return false;
        }

        let mut length_sum: usize = 0;

        for field_index in 0..n_fields {
            let size = self.rec_get_nth_field_size(field_index);
            if !(size < UNIV_PAGE_SIZE || size == UNIV_SQL_NULL) {
                error!("record field {} length {}", field_index, size);
                return false;
            }

            if size == UNIV_SQL_NULL {
                continue;
            } else {
                length_sum += size;
            }
        }

        if length_sum != self.rec_get_data_size() {
            error!(
                "record length should be {} but is {}",
                length_sum,
                self.rec_get_data_size()
            );

            return false;
        }

        true
    }

    /// Get the byte at the provided offset for a field.
    ///
    /// # Arguments
    ///
    /// `n_field`: The nth field.
    /// `offset`: The offset within the field.
    ///
    /// # Returns
    ///
    /// Byte present at the offset.
    pub fn get_nth_field_byte_at_offset(
        &self,
        n_field: usize,
        offset: usize,
    ) -> u8 {
        ut_ad!(self.rec_get_n_fields() > n_field);

        let (field_offset, _size) = self.rec_get_nth_field_offs(n_field);
        unsafe { self.ptr.add(field_offset + offset).read_unaligned() }
    }

    /// Sets a bit field in a 1-byte field of the record.
    ///
    /// # Arguments
    ///
    /// * `val`: The value to set.
    /// * `offset`: The offset from the record pointer.
    /// * `mask`: The bit mask for the field.
    /// * `shift`: The bit shift for the field.
    pub fn rec_set_bit_field_1(
        &self,
        val: usize,
        offset: usize,
        mask: usize,
        shift: usize,
    ) {
        ut_ad!(offset <= REC_N_EXTRA_BYTES);
        ut_ad!((mask >> shift) << shift == mask);
        // ut_ad!((val << shift) & mask == (val << shift));

        let buf =
            unsafe { slice::from_raw_parts_mut(self.ptr.sub(offset), 1) };

        let val =
            (mach_read_from_1(buf) & !(mask as u8)) | (val << shift) as u8;
        mach_write_to_1(buf, val);
    }

    /// Gets a bit field from a 1-byte field of the record.
    ///
    /// # Arguments
    ///
    /// * `offset`: The offset from the record pointer.
    /// * `mask`: The bit mask for the field.
    /// * `shift`: The bit shift for the field.
    ///
    /// # Returns
    ///
    /// The value of the bit field.
    pub fn rec_get_bit_field_1(
        &self,
        offset: usize,
        mask: usize,
        shift: usize,
    ) -> usize {
        ut_ad!(offset <= REC_N_EXTRA_BYTES);
        ut_ad!((mask >> shift) << shift == mask);

        let buf = unsafe { slice::from_raw_parts(self.ptr.sub(offset), 1) };
        let val = mach_read_from_1(buf);
        (val as usize & mask) >> shift
    }

    /// Sets a bit field in a 2-byte field of the record.
    ///
    /// # Arguments
    ///
    /// * `val`: The value to set.
    /// * `offset`: The offset from the record pointer.
    /// * `mask`: The bit mask for the field.
    /// * `shift`: The bit shift for the field.
    pub fn rec_set_bit_field_2(
        &self,
        val: usize,
        offset: usize,
        mask: usize,
        shift: usize,
    ) {
        ut_ad!(offset <= REC_N_EXTRA_BYTES);
        ut_ad!((mask >> shift) << shift == mask);
        ut_ad!((val << shift) & mask == (val << shift));

        let buf =
            unsafe { slice::from_raw_parts_mut(self.ptr.sub(offset), 2) };

        let val =
            (mach_read_from_2(buf) & !(mask as u16)) | (val << shift) as u16;
        mach_write_to_2(buf, val);
    }

    /// Gets a bit field from a 2-byte field of the record.
    ///
    /// # Arguments
    ///
    /// * `offset`: The offset from the record pointer.
    /// * `mask`: The bit mask for the field.
    /// * `shift`: The bit shift for the field.
    ///
    /// # Returns
    ///
    /// The value of the bit field.
    pub fn rec_get_bit_field_2(
        &self,
        offset: usize,
        mask: usize,
        shift: usize,
    ) -> usize {
        ut_ad!(offset <= REC_N_EXTRA_BYTES);
        ut_ad!((mask >> shift) << shift == mask);

        let buf = unsafe { slice::from_raw_parts(self.ptr.sub(offset), 2) };
        let val = mach_read_from_2(buf);
        (val as usize & mask) >> shift
    }
}
