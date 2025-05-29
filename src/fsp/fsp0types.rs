/// The ID of the system tablespace.
///
/// The system tablespace (ID 0) is the main tablespace that contains
/// system data structures and can also contain user data. It is always
/// present in an InnoDB instance.
pub const SYS_TABLESPACE: usize = 0;
