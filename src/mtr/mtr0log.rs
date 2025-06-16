use crate::mach::mach0data::{
    mach_write_compressed, mach_write_to_1, mach_write_to_2, mach_write_to_4,
    mach_write_to_8,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::mtr::mtr0types::MlogType;
use crate::mtr::mtr0types::MtrLogMode::MTR_LOG_NONE;
use crate::ut_panic;

pub struct MLog {
    pub buf: Vec<u8>,
}

impl MLog {
    pub fn new() -> Self {
        MLog { buf: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }
    pub fn append(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Write 1-4 bytes to file page buffered in the buffer pool. And, write the corresponding log recording to the mini-transaction log.
    ///
    /// # Arguments
    ///
    /// * `page` - The page to write to.
    /// * `val` - The value to write.
    /// * `mlog_type` - The type of log to write.
    /// * `mtr` - The mini-transaction to write to.
    ///
    /// Write 1, 2, or 4 bytes at `offset` in `page`, and emit a corresponding log record.
    pub fn mlog_write_u32(
        &mut self,
        page: &mut [u8],
        offset: usize,
        val: usize,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        // 1) apply the change in-memory
        match mlog_type {
            MlogType::MLOG_1BYTE => {
                mach_write_to_1(&mut page[offset..offset + 1], val as u8)
            }
            MlogType::MLOG_2BYTES => {
                mach_write_to_2(&mut page[offset..offset + 2], val as u16)
            }
            MlogType::MLOG_4BYTES => {
                mach_write_to_4(&mut page[offset..offset + 4], val as u32)
            }
            _ => ut_panic!("Invalid mlog type for u32 write: {:?}", mlog_type),
        }
        // 2) record in the MTR log buffer: [type][offset (u16)][value (u32)]
        self.buf.push(mlog_type as u8);
        self.buf.extend_from_slice(&(offset as u16).to_le_bytes());
        self.buf.extend_from_slice(&(val as u32).to_le_bytes());
        mtr.inc_n_log_records();
    }
    /// Write an 8-byte LE value at `offset` in `page` and log it exactly like
    /// C++ `mlog_write_uint64` does in this fork.
    pub fn mlog_write_uint64(
        &mut self,
        page: &mut [u8],
        offset: usize,
        val: usize,
        mtr: &mut Mtr,
    ) {
        // 1) Apply the change in-memory (pages are big-endian)
        mach_write_to_8(&mut page[offset..offset + 8], val as u64);

        // 2) Emit the 1-byte record header (and bump record count)
        self.mlog_write_initial_log_record_fast(MlogType::MLOG_8BYTES, mtr);

        // 3) 2-byte BIG-ENDIAN page offset
        self.buf.extend_from_slice(&(offset as u16).to_le_bytes());

        // 4) Compressed write of the high 32 bits (varint)
        let high = (val >> 32) as u32;
        let mut tmp = [0u8; 5];
        let n = mach_write_compressed(&mut tmp, high);
        self.buf.extend_from_slice(&tmp[..n]);

        // 5) Append the low 32 bits in Little-ENDIAN
        let low = (val as u32).to_le_bytes();
        self.buf.extend_from_slice(&low);
    }

    /// Write a string into the buffer page and record it as a typed redo record,
    /// matching C++ `mlog_write_string` + `mlog_log_string` sequence.
    pub fn mlog_write_string(
        &mut self,
        page: &mut [u8],
        offset: usize,
        data: &[u8],
        mtr: &mut Mtr,
    ) {
        // Copy into page memory
        page[offset..offset + data.len()].copy_from_slice(data);
        // Now log it with 2-byte offset/length + raw bytes
        self.mlog_log_string(offset, data.len(), mtr, page);
    }

    /// Log an in-page string change: [MLOG_WRITE_STRING][2B offset][2B length][data...]
    /// Mirrors C++ `mlog_log_string(ptr, len, mtr)`.
    pub fn mlog_log_string(
        &mut self,
        offset: usize,
        len: usize,
        mtr: &mut Mtr,
        page: &[u8],
    ) {
        // 1) emit record type
        self.buf.push(MlogType::MLOG_WRITE_STRING as u8);
        // 2) 2-byte little-endian offset
        let off_bytes = (offset as u16).to_le_bytes();
        self.buf.extend_from_slice(&off_bytes);
        // 3) 2-byte little-endian length
        let len_bytes = (len as u16).to_le_bytes();
        self.buf.extend_from_slice(&len_bytes);
        // count this as one redo record
        mtr.inc_n_log_records();
        // 4) append the actual string bytes
        self.buf.extend_from_slice(&page[offset..offset + len]);
    }

    /// Emit the end-of-sequence marker for multi-record writes.
    pub fn mlog_multi_rec_end(&mut self, mtr: &mut Mtr) {
        self.buf.push(MlogType::MLOG_MULTI_REC_END as u8);
        mtr.inc_n_log_records();
    }

    /// Append (concatenate) a string into the MTR log, if logging is enabled.
    pub fn mlog_catenate_string(mtr: &mut Mtr, data: &[u8]) {
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }
        mtr.log_buffer_mut().append(data);
    }

    /// Write the initial part of the log record (3 to 11  bytes).
    ///
    /// # Arguments
    ///
    /// * `page` - The page to write to.
    /// * `mlog_type` - The type of log to write.
    /// * `mtr` - The mini-transaction to write to.
    ///
    pub fn mlog_write_initial_log_record_fast(
        &mut self,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        self.buf.resize(self.buf.len() + 1, 0);

        mach_write_to_1(&mut self.buf[0..1], mlog_type as u8);
        mtr.inc_n_log_records();
    }
}
