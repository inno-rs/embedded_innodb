#[derive(Eq, PartialEq)]
pub enum ReadWriteLatch {
    SLatch = 1,
    XLatch = 2,
    NoLatch = 3,
}

/// Acquires an exclusive lock on a block's read-write lock.
///
/// This macro takes a block that has a `rw_lock` field and acquires an exclusive lock on it.
/// The exclusive lock ensures that no other thread can access the block while the lock is held.
///
/// # Parameters
/// * `$block` - A reference to a block that has a `rw_lock` field
///
/// # Safety
/// This macro uses unsafe code to access the raw lock. The caller must ensure that:
/// * The block has a valid `rw_lock` field
/// * The lock is properly released when no longer needed
///
/// # Example
/// ```
/// let block_ref = block.write().unwrap();
/// lock_exclusive!(block_ref);
/// // Now you have exclusive access to the block
/// ```
#[macro_export]
macro_rules! lock_exclusive {
    ($block:expr) => {{
        use parking_lot::RawRwLock;
        let raw_lock: &RawRwLock = unsafe { $block.rw_lock.raw() };
        raw_lock.lock_exclusive();
    }};
}

/// Acquires a shared lock on a block's read-write lock.
///
/// This macro takes a block that has a `rw_lock` field and acquires a shared lock on it.
/// The shared lock allows multiple threads to read the block simultaneously, but prevents
/// any thread from acquiring an exclusive lock for writing.
///
/// # Parameters
/// * `$block` - A reference to a block that has a `rw_lock` field
///
/// # Safety
/// This macro uses unsafe code to access the raw lock. The caller must ensure that:
/// * The block has a valid `rw_lock` field
/// * The lock is properly released when no longer needed
///
/// # Example
/// ```
/// let block_ref = block.write().unwrap();
/// lock_shared!(block_ref);
/// // Now you have shared access to the block
/// ```
#[macro_export]
macro_rules! lock_shared {
    ($block:expr) => {{
        use parking_lot::RawRwLock;
        let raw_lock: &RawRwLock = unsafe { $block.rw_lock.raw() };
        raw_lock.lock_shared();
    }};
}
