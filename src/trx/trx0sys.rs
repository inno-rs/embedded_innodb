use crate::fsp::fsp0types::SYS_TABLESPACE;

/// The tablespace ID where the transaction system is stored.
///
/// This constant defines the tablespace that contains transaction system
/// data structures such as the transaction system header, rollback segments,
/// and undo logs. It is set to the system tablespace by default.
pub const TRX_SYS_SPACE: usize = SYS_TABLESPACE;
