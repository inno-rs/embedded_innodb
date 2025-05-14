use crate::mach::mach0data::{
    mach_write_to_1, mach_write_to_2, mach_write_to_4,
};
use crate::mtr::mtr0mtr::Mtr;
use crate::mtr::mtr0types::MlogType;
use crate::page::page0page::Page;

pub struct MLog {
    buf: Vec<u8>,
}

impl MLog {
    pub fn new() -> Self {
        Self { buf: vec![0; 0] }
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
    pub fn mlog_write_u32(
        &mut self,
        page: &mut Vec<u8>,
        val: usize,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        match mlog_type {
            MlogType::MLOG_1BYTE => {
                mach_write_to_1(&mut page[0..1], val);
            }
            MlogType::MLOG_2BYTES => {
                mach_write_to_2(&mut page[0..2], val);
            }
            MlogType::MLOG_4BYTES => {
                mach_write_to_4(&mut page[0..4], val);
            }
            _ => {
                panic!("Invalid log type");
            }
        }

        // Check if no logging is needed. Return?
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
        page: &Page,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        self.buf.resize(self.buf.len() + 1, 0);

        mach_write_to_1(&mut self.buf[0..1], mlog_type as usize);
        mtr.inc_n_log_records();
    }
}
