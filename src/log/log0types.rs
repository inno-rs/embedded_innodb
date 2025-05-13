use crate::innodb::innodb0types::IB_FILE_BLOCK_SIZE;

/// The counting of the log sequence numbers (LSN) starts from the given value.
pub const LOG_START_LSN: usize = 16 * IB_FILE_BLOCK_SIZE;

/// Offsets to the log block header.

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
