use std::ops::Range;

use crate::innodb::innodb0types::{IB_FILE_BLOCK_SIZE, Lsn, SpaceId};

/// The counting of the log sequence numbers (LSN) starts from the given value.
pub const LOG_START_LSN: Lsn = 16 * IB_FILE_BLOCK_SIZE as Lsn;

// Offsets to the log block header.

/**************************************************************************
 *                InnoDB Redo Log Block Header (12 bytes)                 *
 *       (Found at the start of each 512-byte log block)                  *
 **************************************************************************
 * Offset | Length | Field                         | Description          *
 *--------+--------+-------------------------------+----------------------*
 * 0x00   | 4 B    | LOG_BLOCK_HDR_NO              | Monotonic block #    *
 * 0x04   | 2 B    | LOG_BLOCK_HDR_DATA_LENGTH     | Valid bytes in DATA  *
 * 0x06   | 2 B    | LOG_BLOCK_FIRST_REC_GROUP     | Offset to first rec   *
 * 0x08   | 4 B    | LOG_BLOCK_CHECKPOINT_NO       | Checkpoint # at write*
 *************************************************************************/

/// Block number which must be > 0 and is allowed to wrap around at 2G.
/// The highest bit is set to 1 if this is the first block in a log flush write segment.
pub const LOG_BLOCK_HDR_NO: usize = 0;

/// Mask used to get the highest bit in the field.
pub const LOG_BLOCK_FLUSH_BIT_MASK: usize = 0x80000000;

/// The number of bytes written to the block.
pub const LOG_BLOCK_HDR_DATA_LENGTH: usize = 4;

/// The offset of the first mtr log record group in the log block. If it is 0, it is none.
pub const LOG_BLOCK_FIRST_REC_GROUP: usize = 6;

/// 4 bytes of the checkpoint number.
pub const LOG_BLOCK_CHECKPOINT_NO: usize = 8;

/// Size of the log block header (bytes).
pub const LOG_BLOCK_HDR_SIZE: usize = 12;

/// 4 byte-checksum of the log block contents.
pub const LOG_BLOCK_CHECKSUM: usize = 4;
/// Trailer size in bytes.
pub const LOG_BLOCK_TRL_SIZE: usize = 4;

/// Offsets for checkpoint number.
pub const LOG_CHECKPOINT_NO: usize = 0;
pub const LOG_CHECKPOINT_NO_RANGE: Range<usize> =
    LOG_CHECKPOINT_NO..LOG_CHECKPOINT_NO + 8;
pub const LOG_CHECKPOINT_LSN: usize = 8;
pub const LOG_CHECKPOINT_LSN_RANGE: Range<usize> =
    LOG_CHECKPOINT_LSN..LOG_CHECKPOINT_LSN + 8;
pub const LOG_CHECKPOINT_OFFSET: usize = 16;
pub const LOG_CHECKPOINT_OFFSET_RANGE: Range<usize> =
    LOG_CHECKPOINT_OFFSET..LOG_CHECKPOINT_OFFSET + 4;
pub const LOG_CHECKPOINT_LOG_BUF_SIZE: usize = 20;
pub const LOG_CHECKPOINT_LOG_BUF_SIZE_RANGE: Range<usize> =
    LOG_CHECKPOINT_LOG_BUF_SIZE..LOG_CHECKPOINT_LOG_BUF_SIZE + 4;
pub const LOG_CHECKPOINT_UNUSED_LSN: usize = 24;
pub const LOG_CHECKPOINT_UNUSED_LSN_RANGE: Range<usize> =
    LOG_CHECKPOINT_UNUSED_LSN..LOG_CHECKPOINT_UNUSED_LSN + 8;

pub const LOG_CHECKPOINT_GROUP_ARRAY: usize = 32;

pub const LOG_CHECKPOINT_CHECKSUM_1: usize =
    LOG_CHECKPOINT_GROUP_ARRAY + 32 * 8;
pub const LOG_CHECKPOINT_CHECKSUM_1_RANGE: Range<usize> =
    LOG_CHECKPOINT_CHECKSUM_1..LOG_CHECKPOINT_CHECKSUM_1 + 4;

pub const LOG_CHECKPOINT_1: usize = IB_FILE_BLOCK_SIZE;

/// This is the second location in the log header where we write checkpoint information.
/// We alternate between LOG_CHECKPOINT_1 and LOG_CHECKPOINT_2 to prevent corruption.
pub const LOG_CHECKPOINT_2: usize = 3 * IB_FILE_BLOCK_SIZE;

pub const LOG_FILE_HEADER_SIZE: u64 = 4 * IB_FILE_BLOCK_SIZE as u64;

#[repr(align(512))]
pub struct CheckpointBuf<const SIZE: usize>(pub [u8; SIZE]);

impl<const SIZE: usize> CheckpointBuf<SIZE> {
    pub fn new() -> Self {
        Self { 0: [0; SIZE] }
    }
}

pub enum LogGroupState {
    Ok,
    Corrupted,
}

/// LogGroup consists of a number of log files, each of which is of the same size.
pub struct LogGroup {
    /// Log group id.
    pub id: usize,
    /// Number of files in the log group.
    pub n_files: usize,
    /// Size of the individual file in bytes. This also includes the file header.
    pub file_size: usize,
    /// File space id.
    pub space_id: SpaceId,
    /// State of the log group.
    pub state: LogGroupState,
    /// Lsn used to fix coordinates within the log group.
    pub lsn: Lsn,
    /// The offset of the above lsn.
    pub lsn_offset: u64,
    /// Number of pending flush-writes to this log group.
    pub n_pending_writes: usize,
    /// Buffer to keep checkpoint information.
    pub checkpoint_buf: Box<CheckpointBuf<IB_FILE_BLOCK_SIZE>>,
}
