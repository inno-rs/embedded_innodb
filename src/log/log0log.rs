use crate::innodb::innodb0types::{Lsn, IB_FILE_BLOCK_SIZE};
use crate::log::log0types::{LOG_BLOCK_HDR_SIZE, LOG_BLOCK_TRL_SIZE};

/// Offsets to the log block header.

/// Block number which must be > 0 and is allowed to wrap around at 2G. The highest bit is set to 1 if this is the first block in a log flush write segment.
pub const LOG_BLOCK_HDR_NO: usize = 0;

/// The number of bytes written to the block.
pub const LOG_BLOCK_HDR_DATA_LENGTH: usize = 4;

/// Database log
pub struct Log {
    /// The log sequence number.
    lsn: Lsn,
    /// First free offset in the log buffer.
    buf_free: usize,
    /// The log buffer.
    buf: Vec<u8>,
}

pub struct LogWriteResult {
    start_lsn: Lsn,
    end_lsn: Lsn,
}

impl Log {
    pub fn new(buffer_size: usize) -> Self {
        debug_assert!(
            buffer_size != 0 && buffer_size % IB_FILE_BLOCK_SIZE == 0
        );

        Log {
            lsn: 0,
            buf_free: 0,
            buf: vec![0; buffer_size], // Initialize with a buffer of size 1024
        }
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
        let start = block_offset;
        let end = block_offset + LOG_BLOCK_HDR_NO;
        debug_assert!(end - start == 4);
        self.buf[start..end]
            .copy_from_slice(&(log_block_number as u32).to_le_bytes());
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
        let start = block_offset;
        let end = block_offset + LOG_BLOCK_HDR_DATA_LENGTH + 2;
        self.buf[start..end].copy_from_slice(&(data_len as u16).to_le_bytes());
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

        let data_len = (len + self.buf_free) % IB_FILE_BLOCK_SIZE;

        if data_len > IB_FILE_BLOCK_SIZE - LOG_BLOCK_TRL_SIZE {
            /* The string does not within fit within the current block. */
            return Err("Data length exceeds buffer size".to_string());
        }

        let start_lsn = self.lsn;

        // Copy the data into the buffer
        self.buf[self.buf_free..len].copy_from_slice(data);

        let log_block =
            (self.buf_free / IB_FILE_BLOCK_SIZE) * IB_FILE_BLOCK_SIZE;
        self.block_set_data_length(log_block, len);

        self.buf_free += len;
        self.lsn += len as Lsn;

        Ok(LogWriteResult { start_lsn, end_lsn: self.lsn })
    }
}
