/// The ID of the system tablespace.
///
/// The system tablespace (ID 0) is the main tablespace that contains
/// system data structures and can also contain user data. It is always
/// present in an InnoDB instance.
pub const SYS_TABLESPACE: usize = 0;

/// Flags for inserting records in order.
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Alphabetically Up.
    FspUp = 111,
    /// Alphabetically Down.
    FspDown = 112,
    /// No Order.
    FspNoDir = 113,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FSegId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExtentNo(pub u32);
