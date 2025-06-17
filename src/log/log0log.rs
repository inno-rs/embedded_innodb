use std::sync::{Arc, Condvar, Mutex};

use tracing::error;

use crate::fil::fil0fil::Fil;
use crate::innodb::innodb0types::{IB_FILE_BLOCK_SIZE, Lsn, PageNo, SpaceId};
use crate::log::log0types::{
    LOG_BLOCK_CHECKPOINT_NO, LOG_BLOCK_FIRST_REC_GROUP,
    LOG_BLOCK_FLUSH_BIT_MASK, LOG_BLOCK_HDR_DATA_LENGTH, LOG_BLOCK_HDR_NO,
    LOG_BLOCK_HDR_SIZE, LOG_BLOCK_TRL_SIZE, LOG_START_LSN,
};
use crate::os::os0file::IORequest;
use crate::ut::ut0dbg::ut_ad;

pub const LOG_SPACE_FIRST_ID: usize = 0xFFFFFFF0;

/// We write to alternative checkpoint blocks when we make new checkpoints.
/// This is to avoid overwriting the previous checkpoint blocks resulting in corruption.
pub const LOG_CHECKPOINT_1: usize = IB_FILE_BLOCK_SIZE;
pub const LOG_CHECKPOINT_2: usize = IB_FILE_BLOCK_SIZE * 3;

pub enum LogWaitMode {
    LogNoWait,
    LogWait,
}

/// Database log
pub struct Log {
    /// Reference to fil.
    fil: Arc<Mutex<Fil>>,
    /// Mutex to protect the log.
    mutex: Arc<Mutex<()>>,
    /// True when the log is flushed.
    flushed: bool,
    /// Condition variable to signal when the log is flushed.
    flushed_event: Arc<Condvar>,
    /// The log sequence number.
    lsn: Lsn,
    /// First free offset in the log buffer.
    buf_free: usize,
    /// The log buffer.
    buf: Vec<u8>,
    /// Log buffer size in bytes.
    buf_size: usize,
    /// Recommended maximum value of buf_free.
    max_buf_free: usize,
    /// First offset in the log buffer to write to.
    buf_next_to_write: usize,
    /// End Lsn of the current running write.
    write_lsn: Lsn,
    /// Number of i/o initiated so far.
    n_log_ios: usize,
    /// The data in the buffer has been written to this offset
    /// when the current write operation is finished.
    write_end_offset: Lsn,
    /// End lsn of the current running write and flush operation.
    current_flush_lsn: Lsn,
    /// All the logs before this LSN are flushed to disk.
    flushed_to_disk_lsn: Lsn,
    /// Number of currently pending flushes or writes.
    n_pending_writes: usize,
    /// The next checkpoint number.
    next_checkpoint_number: usize,
    /// Latest checkpoint LSN.
    last_checkpoint_lsn: Lsn,
    /// Next checkpoint LSN.
    next_checkpoint_lsn: Lsn,
    /// Number of pending checkpoints writes.
    n_pending_checkpoint_writes: usize,
}

pub struct LogWriteResult {
    pub start_lsn: Lsn,
    pub end_lsn: Lsn,
}

impl Log {
    pub fn new(buffer_size: usize, fil: Arc<Mutex<Fil>>) -> Self {
        ut_ad!(buffer_size != 0 && buffer_size % IB_FILE_BLOCK_SIZE == 0);

        Log {
            fil,
            mutex: Arc::from(Mutex::new(())),
            flushed: false,
            flushed_event: Default::default(),
            lsn: LOG_START_LSN as Lsn,
            buf_free: LOG_BLOCK_HDR_SIZE,
            buf: vec![0; buffer_size], // Initialize with a buffer of size 1024
            buf_size: buffer_size,
            max_buf_free: 0,
            buf_next_to_write: 0,
            write_lsn: 0,
            n_log_ios: 0,
            write_end_offset: 0,
            current_flush_lsn: 0,
            flushed_to_disk_lsn: 0,
            n_pending_writes: 0,
            next_checkpoint_number: 0,
            last_checkpoint_lsn: 0,
            next_checkpoint_lsn: 0,
            n_pending_checkpoint_writes: 0,
        }
    }

    /// Gets the flush bit from the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The flush bit from the block header.
    pub fn block_get_flush_bit(&self, block_offset: usize) -> bool {
        let block_number = self.block_get_hdr_no(block_offset);
        (block_number & LOG_BLOCK_FLUSH_BIT_MASK) != 0
    }

    /// Sets the flush bit in the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    /// * `flush_bit` - The flush bit to set.
    ///
    /// # Returns
    ///
    /// * None
    pub fn block_set_flush_bit(
        &mut self,
        block_offset: usize,
        flush_bit: bool,
    ) {
        // Set the flush bit in the header
        let block_number = self.block_get_hdr_no(block_offset);
        let new_block_number = if flush_bit {
            block_number | LOG_BLOCK_FLUSH_BIT_MASK
        } else {
            block_number & !LOG_BLOCK_FLUSH_BIT_MASK
        };

        self.block_set_hdr_no(block_offset, new_block_number);
    }

    /// Gets the block number from the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The block number from the header.
    ///
    pub fn block_get_hdr_no(&self, block_offset: usize) -> usize {
        // Get the block number from the header
        let start = block_offset + LOG_BLOCK_HDR_NO;
        let end = block_offset + LOG_BLOCK_HDR_NO + 4;
        let block_number_bytes = &self.buf[start..end];
        u32::from_le_bytes(block_number_bytes.try_into().unwrap()) as usize
    }

    /// Sets the block number in the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    /// * `log_block_number` - The block number to set.
    ///
    /// # Returns
    ///
    /// * None
    pub fn block_set_hdr_no(
        &mut self,
        block_offset: usize,
        log_block_number: usize,
    ) {
        // Set the block number in the header
        let start = block_offset + LOG_BLOCK_HDR_NO;
        let end = block_offset + LOG_BLOCK_HDR_NO + 4;
        debug_assert!(end - start == 4);
        self.buf[start..end]
            .copy_from_slice(&(log_block_number as u32).to_le_bytes());
    }

    /// Gets the data length of the log block.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The data length of the block.
    ///
    pub fn block_get_data_length(&self, block_offset: usize) -> usize {
        // Get the data length from the block
        let start = block_offset + LOG_BLOCK_HDR_DATA_LENGTH;
        let end = block_offset + LOG_BLOCK_HDR_DATA_LENGTH + 4;
        let data_length_bytes = &self.buf[start..end];
        u32::from_le_bytes(data_length_bytes.try_into().unwrap()) as usize
    }

    /// Set the data length of the log block.
    ///
    /// # Arguments
    ///
    /// * `block` - The log block to set the data length for.
    /// * `data_len` - The length of the data to set.
    ///
    /// # Returns
    ///
    /// * None
    pub fn block_set_data_length(
        &mut self,
        block_offset: usize,
        data_len: usize,
    ) {
        // Set the data length in the block
        let start = block_offset + LOG_BLOCK_HDR_DATA_LENGTH;
        let end = block_offset + LOG_BLOCK_HDR_DATA_LENGTH + 4;
        self.buf[start..end].copy_from_slice(&(data_len as u32).to_le_bytes());
    }

    /// Gets the offset of the first mtr log record in a lo
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The offset of the first mtr log record in the block.
    ///
    pub fn block_get_first_rec_group(&self, block_offset: usize) -> usize {
        let start = block_offset + LOG_BLOCK_FIRST_REC_GROUP;
        let end = block_offset + LOG_BLOCK_FIRST_REC_GROUP + 2;
        let first_rec_group_bytes = &self.buf[start..end];
        u16::from_le_bytes(first_rec_group_bytes.try_into().unwrap()) as usize
    }

    /// Sets the offset of the first mtr log record in a log block.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    /// * `rec_offset` - The offset of the first mtr log record to set.
    ///
    /// # Returns
    ///
    /// * None
    pub fn block_set_first_rec_group(
        &mut self,
        block_offset: usize,
        rec_offset: usize,
    ) {
        // Set the offset of the first mtr log record in the block
        let start = block_offset + LOG_BLOCK_FIRST_REC_GROUP;
        let end = block_offset + LOG_BLOCK_FIRST_REC_GROUP + 2;
        self.buf[start..end]
            .copy_from_slice(&(rec_offset as u16).to_le_bytes());
    }

    /// Gets the checkpoint number from the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The checkpoint number from the block header.
    pub fn block_get_checkpoint_number(&self, block_offset: usize) -> usize {
        // Get the checkpoint number from the block
        let start = block_offset + LOG_BLOCK_CHECKPOINT_NO;
        let end = block_offset + LOG_BLOCK_CHECKPOINT_NO + 4;
        let checkpoint_number_bytes = &self.buf[start..end];
        u32::from_le_bytes(checkpoint_number_bytes.try_into().unwrap())
            as usize
    }

    /// Set the checkpoint number fields of the log block header.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    /// * `checkpoint_number` - The checkpoint number to set.
    ///
    /// # Returns
    ///
    /// * None
    pub fn block_set_checkpoint_number(
        &mut self,
        block_offset: usize,
        checkpoint_number: usize,
    ) {
        // Set the checkpoint number in the header
        let start = block_offset + LOG_BLOCK_CHECKPOINT_NO;
        let end = block_offset + LOG_BLOCK_CHECKPOINT_NO + 4;
        self.buf[start..end]
            .copy_from_slice(&(checkpoint_number as u32).to_le_bytes());
    }

    /// Converts a log sequence number (LSN) to a block number.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number to convert.
    ///
    /// # Returns
    ///
    /// * The block number corresponding to the given LSN. It is > 0 and <= 1G.
    ///
    pub fn block_convert_lsn_to_no(lsn: Lsn) -> usize {
        // Convert the LSN to a block number
        ((lsn as usize / IB_FILE_BLOCK_SIZE) & 0x3FFFFFFF) + 1
    }

    /// Calculates the checksum for a log block.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The checksum for the block.
    pub fn block_calc_checksum(&self, block_offset: usize) -> u32 {
        // Calculate the checksum for the block
        let start = block_offset;
        let end = block_offset + IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE;

        let mut sh: u32 = 0;
        let mut sum: u32 = 0;

        for i in start..end {
            let b = self.buf[i] as u32;
            sum &= 0x7FFFFFFF;
            sum += b;
            sum += b << sh;

            sh += 1;
            if sh > 24 {
                sh = 0;
            }
        }

        sum
    }

    /// Retrieve the checksum from the trailer checksum field of the log block.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    ///
    /// # Returns
    ///
    /// * The checksum for the block.
    pub fn block_get_checksum(&self, block_offset: usize) -> u32 {
        let start = block_offset + IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE;
        let end = block_offset + IB_FILE_BLOCK_SIZE;
        let checksum_bytes = &self.buf[start..end];
        u32::from_le_bytes(checksum_bytes.try_into().unwrap())
    }

    /// Store a 4-byte checksum in the trailer checksum field of the log block
    /// before writing it to a log file.
    /// This checksum is used during recovery to check the consistency of the log block.
    pub fn block_store_checksum(&mut self, block_offset: usize) {
        let checksum = self.block_calc_checksum(block_offset);
        let start = block_offset + IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE;
        let end = block_offset + IB_FILE_BLOCK_SIZE;
        self.buf[start..end].copy_from_slice(&checksum.to_le_bytes());
    }

    /// Initializes a log block with the given LSN.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset of the block in the log buffer.
    /// * `lsn` - The log sequence number to set.
    ///
    pub fn block_init(&mut self, block_offset: usize, lsn: Lsn) {
        let block_no = Self::block_convert_lsn_to_no(lsn);

        self.block_set_hdr_no(block_offset, block_no);
        self.block_set_data_length(block_offset, LOG_BLOCK_HDR_SIZE);
        self.block_set_first_rec_group(block_offset, 0);
    }

    /// Reserves space in the log buffer and writes the data to it.
    ///
    /// # Arguments
    ///
    /// * `data` - The data to write to the log buffer.
    ///
    /// # Returns
    ///
    /// * A result containing the start and end LSN of the written data.
    pub fn reserve_and_write_fast(
        &mut self,
        data: &[u8],
    ) -> Result<LogWriteResult, String> {
        let len = data.len();

        let data_len = len + self.buf_free % IB_FILE_BLOCK_SIZE;

        if data_len > IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE {
            /* The string does not within fit within the current block. */
            return Err("Data length exceeds block size".to_string());
        }

        let start_lsn = self.lsn;

        // Copy the data into the buffer
        let range = self.buf_free..self.buf_free + len;
        self.buf[range].copy_from_slice(data);

        let log_block =
            (self.buf_free / IB_FILE_BLOCK_SIZE) * IB_FILE_BLOCK_SIZE;
        self.block_set_data_length(log_block, len);

        self.buf_free += len;
        self.lsn += len as Lsn;

        Ok(LogWriteResult { start_lsn, end_lsn: self.lsn })
    }

    /// Write the given data to the log buffer.
    ///
    /// # Arguments
    ///
    /// `data`: The data to write.
    ///
    /// # Returns
    ///
    /// `Result<LogWriteResult, String>` containing the result of the operation.
    /// If successful, the result contains the start lsn and the end lsn.
    /// Otherwise, error string.
    pub fn write_low(
        &mut self,
        data: &[u8],
    ) -> Result<LogWriteResult, String> {
        let start_lsn = self.lsn;
        let mut str_len = data.len();
        let mut dest_offset: usize = 0;
        while str_len > 0 {
            let mut data_len = (self.buf_free % IB_FILE_BLOCK_SIZE) + str_len;

            let mut len = if data_len < IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE
            {
                // Data fits in the current block.
                str_len
            } else {
                data_len = IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE;
                IB_FILE_BLOCK_SIZE
                    - (self.buf_free % IB_FILE_BLOCK_SIZE)
                    - LOG_BLOCK_TRL_SIZE
            };

            // copy len amount of data to log buffer.
            let src_range = self.buf_free..self.buf_free + len;
            let dest_range = dest_offset..dest_offset + len;
            self.buf[src_range].copy_from_slice(&data[dest_range]);

            str_len -= len;
            dest_offset += len;

            let block_offset = self.buf_free / IB_FILE_BLOCK_SIZE;
            self.block_set_data_length(block_offset, len);

            if data_len == IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE {
                // This block is full.
                // When the block is full, we put the data length to be the size of the whole block.
                // Otherwise, just the amount of actual data without header and trailer.
                // Verify this behavior when reading logs.
                self.block_set_data_length(block_offset, IB_FILE_BLOCK_SIZE);
                self.block_set_checkpoint_number(
                    block_offset,
                    self.next_checkpoint_number,
                );
                len += LOG_BLOCK_HDR_SIZE + LOG_BLOCK_TRL_SIZE;

                self.lsn += len as Lsn;

                // Initialize a new block.
                self.block_init(block_offset + 1, self.lsn);
            } else {
                self.lsn += len as Lsn;
            }

            self.buf_free += len;

            ut_ad!(self.buf_free < self.buf_size);
        }

        Ok(LogWriteResult { start_lsn, end_lsn: self.lsn })
    }

    /// Gets the current lsn.
    ///
    /// # Returns
    ///
    /// * The current log sequence number.
    pub fn get_lsn(&self) -> Lsn {
        self.lsn
    }

    /// Tries to establish a big enough margin of free space in the log buffer
    /// such that new log entries can be concatenated without the need for a flush.
    pub fn flush_margin(&mut self) {
        let lsn = if self.buf_free > self.max_buf_free {
            if self.n_pending_writes > 0 {
                // A flush is already running. Hopefully, it will provide the necessary free space.
                0
            } else {
                self.lsn
            }
        } else {
            0
        };

        if lsn > 0 {
            self.write_up_to(lsn, LogWaitMode::LogNoWait, false);
        }
    }

    pub fn group_write_buf(
        &mut self,
        block_offset: usize,
        len: usize,
        start_lsn: Lsn,
    ) {
        ut_ad!(start_lsn % IB_FILE_BLOCK_SIZE as u64 == 0);
        ut_ad!(len % IB_FILE_BLOCK_SIZE == 0);

        for i in 0..(len / IB_FILE_BLOCK_SIZE) {
            self.block_store_checksum(block_offset + i)
        }

        self.n_log_ios += 1;

        let mut fil = self.fil.lock().unwrap();
        let io_result = fil.io(
            IORequest::AsyncLogWrite,
            false,
            LOG_SPACE_FIRST_ID as SpaceId,
            block_offset as PageNo,
            0,
            self.buf.as_mut_slice(),
            len,
        );

        match io_result {
            Ok(r) => {}
            Err(e) => {
                error!("io error: {}", e);
            }
        }
    }

    /// Checks if the log is written upto the given LSN.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number up to which lsn should be written.
    /// * `log_wait` - The log wait mode.
    /// * `flush_to_disk` - Whether to flush the log to disk.
    ///
    pub fn write_up_to(
        &mut self,
        lsn: Lsn,
        log_wait: LogWaitMode,
        flush_to_disk: bool,
    ) {
        let do_wait = || match log_wait {
            LogWaitMode::LogWait => {
                return;
            }
            LogWaitMode::LogNoWait => {
                return;
            }
        };

        loop {
            // We are already past the lsn being checked.
            if flush_to_disk && self.flushed_to_disk_lsn >= lsn {
                return;
            }

            if self.n_pending_writes > 0 {
                // A write(+ possibly flush) is running.

                if flush_to_disk && self.current_flush_lsn >= lsn {
                    do_wait();
                    return;
                }

                if !flush_to_disk && self.write_lsn >= lsn {
                    // The current write operation is enough. Wait for it to finish.
                    do_wait();
                    return;
                }
            }

            if !flush_to_disk && self.buf_free == self.buf_next_to_write {
                // Nothing to write, and no flush to disk requested.
                return;
            }

            self.n_pending_writes += 1;

            let start_offset = self.buf_free;
            let end_offset = self.buf_next_to_write;

            let area_start =
                (start_offset / IB_FILE_BLOCK_SIZE) * IB_FILE_BLOCK_SIZE;
            let area_end =
                (end_offset / IB_FILE_BLOCK_SIZE) * (IB_FILE_BLOCK_SIZE + 1);

            ut_ad!(area_start % IB_FILE_BLOCK_SIZE == 0);
            ut_ad!(area_end - area_start > 0);

            self.write_lsn = self.lsn;

            if flush_to_disk {
                self.current_flush_lsn = self.lsn;
            }

            self.buf_free += IB_FILE_BLOCK_SIZE;

            self.group_write_buf(area_start, area_end, 0);

            self.n_pending_writes -= 1;
        }
    }

    /// Checks if a flush operation is completed and does the completion work if yes.
    pub fn sys_check_flush_completion(&mut self) {
        if self.n_pending_writes == 0 {
            if self.write_end_offset > (self.max_buf_free / 2) as u64 {
                // Move the log content to the beginning of the buffer.
                let move_start = (self.write_end_offset as usize
                    / IB_FILE_BLOCK_SIZE)
                    * IB_FILE_BLOCK_SIZE;
                let move_end = (self.buf_free / IB_FILE_BLOCK_SIZE)
                    * (IB_FILE_BLOCK_SIZE + 1);

                self.buf.copy_within(move_start..move_end, 0);
                self.buf_free -= move_start;
                self.buf_next_to_write -= move_start;
            }
        }
    }

    /// Advances the lsn to the least lsn greater than the given lsn.
    /// All the pages in the range are flushed to disk.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number to advance to.
    ///
    /// # Returns
    ///
    /// * `true` if the lsn was advanced, `false` otherwise.
    pub fn preflush_pool_modified_page(
        &mut self,
        lsn: Lsn,
        write_always: bool,
    ) -> bool {
        false
    }

    /// Checkpoint the log.
    ///
    /// # Arguments
    ///
    /// * `sync` - Whether to sync the log to disk.
    /// * `write_always` - Whether to write the log even if it is not necessary.
    ///
    /// # Returns
    ///
    /// * `true` if the checkpoint was successful, `false` if the checkpoint was already running.
    ///
    pub fn checkpoint(&mut self, sync: bool, write_always: bool) -> bool {
        false
    }

    /// Makes a checkpoint at the given LSN or later.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number to make the checkpoint at or later lsn.
    /// * `write_always` -
    /// This function usually checks
    /// if the new checkpoint is done at a later lsn than the current one or not.
    /// If `write_always` is true, it will always write the checkpoint.
    pub fn make_checkpoint_at(&mut self, lsn: Lsn, write_always: bool) {
        while !self.preflush_pool_modified_page(lsn, true) {}
        while !self.checkpoint(false, write_always) {}
    }

    /// Completes the checkpoint.
    pub fn complete_checkpoint(&mut self) {
        self.next_checkpoint_number += 1;
        self.last_checkpoint_lsn = self.next_checkpoint_lsn;
    }

    /// Completes an asynchronous checkpoint.
    pub fn io_complete_checkpoint(&mut self) {
        debug_assert!(self.n_pending_checkpoint_writes > 0);

        self.n_pending_checkpoint_writes -= 1;

        if self.n_pending_checkpoint_writes == 0 {
            self.complete_checkpoint();
        }
    }

    #[cfg(debug_assertions)]
    pub fn get_log_buffer(&mut self) -> Vec<u8> {
        self.buf.clone()
    }
}
