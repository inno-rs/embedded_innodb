use crate::sync::sync0rw::ReadWriteLatch::{NoLatch, SLatch, XLatch};

/// BTree insert mode.
pub const BTR_INSERT: usize = 512;

/// Flag indicating that the operation should estimate the number of rows.
pub const BTR_ESTIMATE: usize = 1024;

/// Represents the different latch modes used during B-tree operations.
///
/// This enum defines the various latching strategies applied when accessing
/// or modifying B-tree nodes. Latches are used to ensure thread safety
/// during concurrent operations on the B-tree structure.
#[repr(usize)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtrLatchMode {
    /// Search the record on the leaf, and S-Latch it.
    SearchLeaf = SLatch as usize,
    /// Modify the leaf record and X-Latch it.
    ModifyLeaf = XLatch as usize,
    /// No latches.
    NoLatches = NoLatch as usize,
    /// Start modifying the entire btree.
    ModifyTree = 33,
    /// Continue modifying the entire btree.
    ContModifyTree = 34,
    /// Search the previous record.
    SearchPrev = 35,
    /// Modify the previous record.
    ModifyPrev = 36,
}
