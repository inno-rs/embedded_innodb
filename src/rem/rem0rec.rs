use crate::data::data0types::{DField, DTuple};
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::{UNIV_PAGE_SIZE, UNIV_SQL_NULL};
use crate::page::page0page::Page;
use log::info;
use std::ptr::{null, null_mut};
use std::{ptr, slice};

/// Info bit denoting the predefined minimum record:
/// this bit is set if and only if the record
/// is the first user record on a non-leaf B-Tree page that is the leftmost page on its level.
pub const REC_INFO_MIN_REC_FLAG: usize = 0x10;

/** Number of extra bytes in a record in addition to the data and offsets. */
/** Record status values */
pub const REC_STATUS_ORDINARY: usize = 0;
pub const REC_STATUS_NODE_PTR: usize = 1;
pub const REC_STATUS_INFIMUM: usize = 2;
pub const REC_STATUS_SUPREMUM: usize = 3;

pub const REC_N_EXTRA_BYTES: usize = 6;

/// Maximum length of the data in the physical record if the offsets are in 1-byte format.
pub const REC_1BYTE_OFFS_LIMIT: usize = 0x7F;

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
║ -4            ║ [6..7] → Number of fields (bits 6–7)          ║
║               ║ [0..5] → Heap number (bits 0–5)              ║
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

/// Bit to specify that the column is stored externally.
pub const REC_OFFS_EXTERNAL: usize = 1 << 30;

/// Record data enum.
pub enum RecordData {
    Data(String),
    SqlNullData(),
}

#[derive(PartialEq, Debug, Copy, Clone)]
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
        let area_start = REC_N_EXTRA_BYTES + n_fields;
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

    pub fn rec_copy(&self, copy_to: &mut Record) {
        let data_size = self.rec_get_data_size();
        let extra_len = REC_N_EXTRA_BYTES + self.rec_get_n_fields();

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
        let align = UNIV_PAGE_SIZE;
        let addr = self.ptr as usize;
        let aligned_addr = addr & !(align - 1); // Align down by masking lower bits
        aligned_addr as *mut u8
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

            let page_ptr = self.page_align();
            let next_ptr = page_ptr.add(offset);
            Some(Record::new(next_ptr as *mut u8))
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
                None => panic!("rec_get_prev: no next record"),
            }
        }

        Some(prev_rec)
    }

    /// Builds a physical record out of a data tuple, and stores it in the record.
    ///
    /// # Arguments
    ///
    /// * `index`: The index to which the record belongs.
    /// * `d_tuple`: The data tuple to convert.
    /// * `n_ext`: The number of external fields.
    ///
    pub fn rec_convert_dtuple_to_rec(
        &self,
        index: &Index,
        mut d_tuple: DTuple,
        n_ext: usize,
    ) -> Record {
        info!("rec_convert_dtuple_to_rec");

        let n_fields = d_tuple.dtuple_get_n_fields();
        let data_size = d_tuple.dtuple_get_data_size();

        /* Calculate the offset of the origin in the physical record */

        /* Store the number of fields */
        self.rec_set_n_fields(n_fields);

        /* Store the info bits of the record */
        self.rec_set_info_bits(d_tuple.dtuple_get_info_bits());

        /* Store the data and offsets */
        let mut end_offset = 0;

        if n_ext == 0 {
            for i in 0..n_fields {
                let field = d_tuple.dtuple_get_nth_field(i);
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
                            ptr::copy_nonoverlapping(
                                field.dfield_get_data(),
                                ptr,
                                length,
                            );
                        }

                        end_offset += length;
                        ored_offset = end_offset;
                    }
                }

                self.rec_1_set_field_end_info(i, ored_offset);
            }
        }

        Record { ptr: self.ptr }
    }

    /// Gets the number of fields in the current record.
    ///
    /// # Returns
    ///
    /// * The number of fields in the current record.
    ///
    pub fn rec_get_n_fields(&self) -> usize {
        unsafe {
            println!("rec_get_n_fields: at={}", self.ptr as usize);
            let p = self.ptr.sub(REC_N_FIELDS) as *const u16;
            let val = p.read_unaligned();
            (val as usize & REC_N_FIELDS_MASK) >> REC_N_FIELDS_SHIFT
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
            println!(
                "rec_set_n_fields: n_fields={} at={}",
                n_fields, self.ptr as usize
            );
            let p = self.ptr.sub(REC_N_FIELDS) as *mut u16;
            let val = (p.read_unaligned() as usize & !REC_N_FIELDS_MASK)
                | (n_fields << REC_N_FIELDS_SHIFT);
            p.write_unaligned(val as u16);
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
            (p.read_unaligned() as usize & REC_N_OWNED_MASK)
                >> REC_N_OWNED_SHIFT
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

    /// Sets the number of records owned by the current directory record.
    ///
    /// # Arguments
    ///
    /// * `n_owned`: The number of records owned to set.
    ///
    /// # Returns
    ///
    /// * `None` if the operation was successful, or an error if it failed.
    ///
    pub fn rec_set_n_owned(&self, n_owned: usize) {
        unsafe {
            let p = self.ptr.sub(REC_N_OWNED);
            let prev_val = p.read_unaligned();
            let val = (prev_val as usize & !REC_N_OWNED_MASK)
                | (n_owned << REC_N_OWNED_SHIFT);
            p.write_unaligned(val as u8);
        }
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
            let prev_val = p.read_unaligned();
            let val = (prev_val as usize & !REC_HEAP_NO_MASK)
                | (heap_no << REC_HEAP_NO_SHIFT);
            p.write_unaligned(val as u16);
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
            let p = (self.ptr.sub(REC_HEAP_NO) as *const u16).read_unaligned();
            (p as usize & REC_HEAP_NO_MASK) >> REC_HEAP_NO_SHIFT
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

    /// Reads the start offset the data field in the record.
    /// The start of the SQL null field is the end offset of the previous non-null field,
    /// or 0. If n is the number of last field + 1,
    /// then the end offset of the last field is returned.
    pub fn rec_get_field_start_offs(&self, field_no: usize) -> usize {
        if field_no == 0 {
            0
        } else {
            self.rec_1_get_field_start_offs(field_no)
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
        if field_no == 0 {
            return 0;
        }

        unsafe {
            let p = self.ptr.sub(REC_N_EXTRA_BYTES + field_no);
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
            panic!(
                "rec_get_nth_field_offs: n_field out of range. fields_count={}",
                fields_count
            );
        }

        let offset = self.rec_1_get_field_start_offs(n_field);
        let next_offset = self.rec_1_get_field_end_info(n_field);

        if next_offset & REC_1BYTE_SQL_NULL_MASK != 0 {
            return (offset, UNIV_SQL_NULL);
        }

        (offset, next_offset - offset)
    }

    pub fn rec_get_nth_field(&self, n_field: usize) -> usize {
        let fields_count = self.rec_get_n_fields();
        if n_field >= fields_count {
            panic!("rec_get_nth_field: n_field out of range");
        }

        0
    }

    /// This function returns the data size of a physical record which is the sum of field lengths. SQL null fields are counted as lengths of zero. The value returned by the function is difference between record origin and record end.
    ///
    /// # Returns
    ///
    ///
    pub fn rec_get_data_size(&self) -> usize {
        self.rec_1_get_field_start_offs(self.rec_get_n_fields())
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
    pub fn rec_get_data(&self, n_field: usize) -> Option<RecordData> {
        let (offset, length) = self.rec_get_nth_field_offs(n_field);
        println!("rec_get_data: offset={}, length={}", offset, length);

        if length == UNIV_SQL_NULL {
            return Some(RecordData::SqlNullData());
        }

        unsafe {
            let bytes = slice::from_raw_parts(self.ptr.add(offset), length);
            let data = std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .ok()
                .unwrap();
            Some(RecordData::Data(data))
        }
    }

    /// Converts a physical record to string.
    ///
    pub fn rec_to_string(&self) -> String {
        let n_fields = self.rec_get_n_fields();
        let info_bits = self.rec_get_info_bits();

        let mut output_builder = String::new();
        output_builder.push_str(&format!(
            "PHYSICAL RECORD: n_fields {}; {}-byte offsets; info bits {}",
            n_fields, n_fields, info_bits
        ));

        for n_field in 0..n_fields {
            output_builder.push_str(&format!(" {}: ", n_field));

            let data = self.rec_get_data(n_field);
            match data {
                None => {
                    panic!("rec_to_string: data out of range");
                }
                Some(d) => match d {
                    RecordData::Data(data) => output_builder
                        .push_str(&format!(" {}; len: {} ", data, data.len())),
                    RecordData::SqlNullData() => {
                        output_builder
                            .push_str(&format!(" SQL NULL, len: {}", 5));
                    }
                },
            }
        }

        output_builder
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
    fn rec_get_converted_extra_size(
        data_size: usize,
        n_fields: usize,
        n_ext: usize,
    ) -> usize {
        if n_ext == 0 && data_size <= REC_1BYTE_OFFS_LIMIT {
            return REC_N_EXTRA_BYTES + n_fields;
        }

        todo!("implement for 2 byte offset")
    }

    /// Returns the total size of the physical record.
    pub fn rec_offs_size(&self) -> usize {
        let n_field = self.rec_get_n_fields();
        let mut end_offset_n_field = self.rec_1_get_field_end_info(n_field);

        if end_offset_n_field & REC_1BYTE_SQL_NULL_MASK
            == REC_1BYTE_SQL_NULL_MASK
        {
            end_offset_n_field &= !REC_1BYTE_SQL_NULL_MASK;
        }

        REC_N_EXTRA_BYTES + n_field + end_offset_n_field
    }
}
