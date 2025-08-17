use std::sync::{Arc, Condvar, Mutex};

use tracing::error;

use crate::{
    fil::fil0fil::Fil,
    innodb::innodb0types::{
        IB_FILE_BLOCK_SIZE, Lsn, PageNo, SpaceId, UNIV_PAGE_SIZE,
    },
    log::log0types::{
        CheckpointBuf, LOG_BLOCK_CHECKPOINT_NO, LOG_BLOCK_FIRST_REC_GROUP,
        LOG_BLOCK_FLUSH_BIT_MASK, LOG_BLOCK_HDR_DATA_LENGTH, LOG_BLOCK_HDR_NO,
        LOG_BLOCK_HDR_SIZE, LOG_BLOCK_TRL_SIZE, LOG_CHECKPOINT_CHECKSUM_1,
        LOG_CHECKPOINT_CHECKSUM_1_RANGE, LOG_CHECKPOINT_LOG_BUF_SIZE_RANGE,
        LOG_CHECKPOINT_LSN_RANGE, LOG_CHECKPOINT_NO_RANGE,
        LOG_CHECKPOINT_OFFSET_RANGE, LOG_CHECKPOINT_UNUSED_LSN_RANGE,
        LOG_FILE_HEADER_SIZE, LOG_START_LSN, LogGroup, LogGroupState,
    },
    mach::mach0data::{mach_write_to_4, mach_write_to_8},
    os::os0file::IORequest,
    srv::srv0srv::Config,
    ut::{ut0dbg::ut_ad, ut0math::ut_align_down, ut0rnd::ut_fold_string},
};

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
    config: Arc<Config>,
    /// Reference to fil.
    fil: Arc<Mutex<Fil>>,
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
    /// Log group associated with log.
    group: LogGroup,
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
    pub fn new(
        buffer_size: usize,
        fil: Arc<Mutex<Fil>>,
        config: Arc<Config>,
    ) -> Self {
        ut_ad!(buffer_size != 0 && buffer_size % IB_FILE_BLOCK_SIZE == 0);
        let n_log_files = config.n_log_files;
        let total_size = config.log_file_size * UNIV_PAGE_SIZE;

        Log {
            config,
            fil,
            flushed: false,
            flushed_event: Default::default(),
            lsn: LOG_START_LSN as Lsn,
            buf_free: LOG_BLOCK_HDR_SIZE,
            buf: vec![0; buffer_size], // Initialize with a buffer of size 1024
            buf_size: buffer_size,
            max_buf_free: 0,
            group: LogGroup {
                id: 0,
                n_files: n_log_files,
                file_size: total_size,
                space_id: 0,
                state: LogGroupState::Ok,
                lsn: LOG_START_LSN,
                lsn_offset: LOG_FILE_HEADER_SIZE,
                n_pending_writes: 0,
                checkpoint_buf: Box::new(CheckpointBuf::new()),
            },
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

    /// Calculate the offset within the group excluding the log file headers.
    /// Since the offset keeps on increasing,
    /// and we are writing logs in a circular fashion between the two log files,
    /// this function helps calculate where the offset will fall.
    ///
    /// # Arguments
    ///
    /// `offset`: The real offset within the group.
    ///
    /// # Returns
    ///
    /// The offset within the log group.
    pub fn group_calc_size_offset(&self, offset: u64) -> u64 {
        offset - (1 + (offset / self.group.file_size as u64))
    }

    /// Calculates the offset within a log group when the log file headers are included.
    ///
    /// # Arguments
    ///
    /// * `offset` - The offset within the log group excluding headers.
    ///
    /// # Returns
    ///
    /// * The real offset within the log group including headers.
    ///
    pub fn group_calc_real_offset(&self, offset: u64) -> u64 {
        offset
            + LOG_FILE_HEADER_SIZE
                * (1 + offset
                    / (self.group.file_size as u64 - LOG_FILE_HEADER_SIZE))
    }

    /// Total capacity of the log group. Log file headers are not included.
    pub fn group_get_capacity(&self) -> u64 {
        (self.group.file_size as u64 - LOG_FILE_HEADER_SIZE)
            * self.group.n_files as u64
    }

    /// Calculates an offset of the lsn within the group.
    pub fn group_calc_lsn_offset(&self) -> u64 {
        let gr_lsn = self.group.lsn;
        let gr_lsn_offset_size = self.group_calc_size_offset(gr_lsn);
        let group_size = self.group_get_capacity();

        let difference = if self.lsn > gr_lsn {
            self.lsn - gr_lsn
        } else {
            let diff = (gr_lsn - self.lsn) % group_size;
            group_size - diff
        };

        let offset = (gr_lsn_offset_size + difference) % group_size;

        self.group_calc_real_offset(offset)
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

    /// Gets the offset of the first mtr log record in a log block.
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

    /// Writes a portion of the log buffer to disk.
    ///
    /// This function calculates checksums for each log block in the specified range,
    /// increments the I/O counter, and initiates an asynchronous write operation
    /// to persist the log data to disk.
    ///
    /// # Arguments
    ///
    /// * `block_offset` - The offset in the log buffer where the write should start.
    /// * `len` - The length of data to write, must be a multiple of the block size.
    /// * `start_lsn` - The LSN corresponding to the start of the write.
    ///
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

    /// Ensures the log is written up to the given LSN.
    ///
    /// This function checks if the log has been written up to the specified LSN.
    /// If not, it initiates a write operation to ensure the log is written up to that point.
    /// Depending on the parameters,
    /// it may also wait for the write to complete and/or flush to disk.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number up to which log should be written.
    /// * `log_wait` - The log wait mode (LogWait or LogNoWait).
    /// * `flush_to_disk` - Whether to flush the log to disk.
    ///
    /// # Returns
    ///
    /// This function does not return a value
    /// but ensures the log is written up to the specified LSN.
    ///
    pub fn write_up_to(
        &mut self,
        target_lsn: Lsn,
        wait: LogWaitMode,
        flush_to_disk: bool,
    ) {
        if flush_to_disk && self.flushed_to_disk_lsn >= target_lsn {
            return;
        }
        if !flush_to_disk && self.write_lsn >= target_lsn {
            return;
        }

        if self.n_pending_writes > 0 {
            if flush_to_disk && self.current_flush_lsn >= target_lsn {
                return; // would be os_event wait in C++
            }
            if !flush_to_disk && self.write_lsn >= target_lsn {
                return; // ditto
            }
            // fallthrough: start our own write below if there’s pending redo
        }

        if self.buf_free <= self.buf_next_to_write {
            return;
        }

        self.n_pending_writes += 1;

        let start_offset = self.buf_next_to_write;
        let end_offset = self.buf_free;

        let area_start = ut_align_down(start_offset, IB_FILE_BLOCK_SIZE);
        let area_end = ut_align_down(end_offset, IB_FILE_BLOCK_SIZE);

        if area_end <= area_start {
            // Nothing to write; bail cleanly (prevents asserts on empty span)
            self.n_pending_writes -= 1;
            return;
        }

        self.write_lsn = self.lsn;
        if flush_to_disk {
            self.current_flush_lsn = self.lsn;
        }

        // InnoDB also sets flush bits + checkpoint no in the log blocks and
        // copies the last partial block upward. Those are optional here.
        // TODO: block_set_flush_bit(..); block_set_checkpoint_no(..);
        // TODO: copy last incomplete log block up one block if you maintain a raw log buffer.

        self.buf_free = self.buf_free.saturating_add(IB_FILE_BLOCK_SIZE);
        self.write_end_offset = self.buf_free as Lsn;

        let lsn_aligned_down: Lsn =
            ut_align_down(self.write_lsn, IB_FILE_BLOCK_SIZE as Lsn);

        self.group_write_buf(area_start, area_end, lsn_aligned_down);

        // Advance the “written” frontier to the end we just flushed.
        self.buf_next_to_write = area_end;

        // If fsync requested, mark durable up to write_lsn.
        if flush_to_disk {
            // (C++ would call srv_fil->flush(...) here; your FS layer may do that elsewhere.)
            self.flushed_to_disk_lsn = self.write_lsn;
        }

        // Done
        self.n_pending_writes -= 1;

        // Note: C++ then enters a section that checks group flush completion,
        // updates counters, and signals events. In this single-threaded port,
        // there’s nothing more to do here.
        let _ = wait; // avoid unused param warning for now
    }

    /// Checks if a flush operation is completed and performs the necessary cleanup.
    ///
    /// This function verifies if there are no pending writes, and if so, it may
    /// move log content to the beginning of the buffer to optimize space usage.
    /// This is particularly useful when a significant portion of the buffer has been
    /// written and can be reused.
    ///
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

    /// Advances the LSN to the least LSN greater than the given LSN and flushes modified pages.
    ///
    /// This function ensures that all modified pages with LSNs up to the specified value
    /// are flushed to disk. It's an important part of the checkpoint mechanism to ensure
    /// data durability.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number to advance to.
    /// * `write_always` - If true, forces the operation even if it might not be necessary.
    ///
    /// # Returns
    ///
    /// * `true` if the LSN was advanced and all pages were flushed, `false` otherwise.
    pub fn preflush_pool_modified_page(
        &mut self,
        lsn: Lsn,
        write_always: bool,
    ) -> bool {
        false
    }

    /// Creates a checkpoint in the log.
    ///
    /// A checkpoint is a point in the log sequence where all modified pages up to that point
    /// have been written to disk. This allows the database to recover from that point in case
    /// of a crash, without having to replay the entire log.
    ///
    /// # Arguments
    ///
    /// * `sync` - If true, synchronously waits for the checkpoint to be written to disk.
    /// * `write_always` - If true, forces writing the checkpoint even if it might not be necessary
    ///   based on LSN comparison with the previous checkpoint.
    ///
    /// # Returns
    ///
    /// * `true` if the checkpoint was successfully initiated, `false` if a checkpoint operation
    ///   was already in progress.
    ///
    pub fn checkpoint(&mut self, sync: bool, write_always: bool) -> bool {
        false
    }

    /// Writes checkpoint information to the log group's checkpoint buffer.
    ///
    /// This method populates the checkpoint buffer with various pieces of information:
    /// 1. The checkpoint number
    /// 2. The checkpoint LSN (Log Sequence Number)
    /// 3. The checkpoint offset (set to 0)
    /// 4. The log buffer size
    /// 5. The unused LSN (set to maximum value)
    ///
    /// # Arguments
    ///
    /// * `log_group` - The log group whose checkpoint buffer will be updated
    pub fn group_checkpoint(&mut self, log_group: &mut LogGroup) {
        // 1. Write the checkpoint number.
        mach_write_to_8(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_NO_RANGE],
            self.next_checkpoint_number as u64,
        );

        // 2. Write the checkpoint lsn.
        mach_write_to_8(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_LSN_RANGE],
            self.next_checkpoint_lsn as u64,
        );

        // 3. Write the checkpoint offset.
        mach_write_to_4(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_OFFSET_RANGE],
            0,
        );

        // 4. Log buffer size.
        mach_write_to_4(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_LOG_BUF_SIZE_RANGE],
            self.buf_size as u32,
        );

        // 5. Write unused LSN
        mach_write_to_8(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_UNUSED_LSN_RANGE],
            u64::MAX,
        );

        // 6. Write Checksum
        let checksum1 = ut_fold_string(
            &log_group.checkpoint_buf.0[0..LOG_CHECKPOINT_CHECKSUM_1],
        );
        mach_write_to_4(
            &mut log_group.checkpoint_buf.0[LOG_CHECKPOINT_CHECKSUM_1_RANGE],
            checksum1,
        );

        // We write the checkpoint information alternatively to each checkpoint place.
        let write_offset = if self.next_checkpoint_number & 1 == 0 {
            LOG_CHECKPOINT_1
        } else {
            LOG_CHECKPOINT_2
        };

        // Checkpoint information is written to the first log file only.
        let write_result = {
            let mut fil_guard = self.fil.lock().unwrap();
            fil_guard.io(
                IORequest::AsyncLogWrite,
                false,
                self.group.space_id,
                (write_offset / UNIV_PAGE_SIZE) as PageNo,
                write_offset % UNIV_PAGE_SIZE,
                &mut self.group.checkpoint_buf.0,
                IB_FILE_BLOCK_SIZE,
            )
        };

        ut_ad!(write_result.is_ok());
    }

    /// Makes a checkpoint at the given LSN or later.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The log sequence number to make the checkpoint at or later lsn.
    /// * `write_always` - Controls whether to force writing the checkpoint.
    ///   This function usually checks if the new checkpoint is done at a later lsn than the current one.
    ///   If `write_always` is true, it will always write the checkpoint regardless of LSN comparison.
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
    pub fn get_log_buffer(&mut self) -> &Vec<u8> {
        &self.buf
    }
}
