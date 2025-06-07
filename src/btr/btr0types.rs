use crate::sync::sync0rw::ReadWriteLatch::{NoLatch, SLatch, XLatch};

/// Represents the different latch modes used during B-tree operations.
///
/// This enum defines the various latching strategies applied when accessing
/// or modifying B-tree nodes. Latches are used to ensure thread safety
/// during concurrent operations on the B-tree structure.
#[repr(usize)]
pub enum BtrLatchMode {
    /// Search the record on the leaf, and S-Latch it.
    BtrSearchLeaf = SLatch as usize,
    /// Modify the leaf record and X-Latch it.
    BtrModifyLeaf = XLatch as usize,
    /// No latches.
    BtrNoLatches = NoLatch as usize,
    /// Start modifying the entire btree.
    BtrModifyTree = 33,
    /// Continue modifying the entire btree.
    BtrContModifyTree = 34,
    /// Search the previous record.
    BtrSearchPrev = 35,
    /// Modify the previous record.
    BtrModifyPrev = 36,
}
