/// The btr module implements B-tree index operations for the embedded InnoDB storage engine.
/// It provides functionality for navigating, searching, and modifying B-tree structures.
pub(crate) mod btr0btr;
mod btr0btr_tests;
pub(crate) mod btr0cur;
mod btr0pcur;
mod btr0types;
