use std::sync::Arc;

use parking_lot::Mutex;


use crate::{
    buf::buf0types::BufBlock,
    log::log0log::Log,
    mach::mach0data::{mach_read_from_1, mach_read_from_2, mach_read_from_4},
    mtr::{
        mtr0log::MLog,
        mtr0types::{
            LatchHandle, LockMode, MemoSlot, MlogType, MtrLogMode, MtrState,
            OtherResource, Savepoint,
        },
    },
    page::page0page::Page,
    ut::ut0dbg::ut_ad,
    ut_panic,
};
use crate::buf::buf0iface::BufPoolLike;

pub const MTR_MAGIC_N: u32 = 54551;
/// Mini-transaction (MTR) for managing atomic operations in the database.
///
/// A mini-transaction is a sequence of database operations that must be
/// performed atomically. It provides mechanisms for latching resources,
/// logging changes, and ensuring that operations can be rolled back if needed.
///
/// The MTR tracks all resources it acquires in a memo structure and manages
/// a log buffer for recording changes that need to be applied to the redo log.
pub struct Mtr {
    mtr_state: MtrState,
    log_mode: MtrLogMode,
    modifications: bool,
    n_log_records: usize,
    memo: Vec<MemoSlot>,
    log_buffer: MLog,
    savepoints: Vec<Savepoint>,
    dblwr_create_in_progress: bool,
    magic_n: u32,
    buf_pool: Option<Arc<dyn BufPoolLike>>,
}

impl Mtr {
    /// Creates a new mini-transaction (MTR) instance.
    ///
    /// This constructor initializes a new MTR with default values:
    /// - State is set to `MtrUndefined`
    /// - Log mode is set to `MTR_LOG_NONE` (no logging)
    /// - No modification flag is set to false
    /// - Log record count is initialized to 0
    /// - Empty memo list for tracking resources
    /// - Empty log buffer
    /// - Empty savepoints list
    /// - a Double-write buffer creation flag is set to false
    /// - Magic number is set to the constant value for validation
    ///
    /// # Returns
    /// A new `Mtr` instance in the undefined state
    pub fn new() -> Self {
        Self {
            mtr_state: MtrState::MtrUndefined,
            log_mode: MtrLogMode::MtrLogNone,
            modifications: false,
            n_log_records: 0,
            memo: Vec::new(),
            log_buffer: MLog::new(),
            savepoints: Vec::new(),
            dblwr_create_in_progress: false,
            magic_n: MTR_MAGIC_N,
            buf_pool: None,
        }
    }

    /// Starts a new mini-transaction by setting its state to ACTIVE
    /// and resetting the log record count.
    /// Start (or restart) this mini‐transaction.
    pub fn start(&mut self) {
        // logging mode → ALL
        self.log_mode = MtrLogMode::MtrLogModeAll;
        // no buffer‐page modifications yet
        self.modifications = false;
        // clear any prior log-record count
        self.n_log_records = 0;
        // now Active
        self.mtr_state = MtrState::MtrActive;

        // clear out any old memo/log data
        self.memo.clear();
        self.log_buffer.clear();
        self.savepoints.clear();

        // debug-only magic number check

        ut_ad!(self.magic_n == MTR_MAGIC_N);
    }

    /// Records a resource in the mini-transaction's memo list so it can be tracked and released later.
    ///
    /// This method adds a resource (like a page fix, latch, etc.) to the memo list,
    /// which keeps track of all resources acquired during the mini-transaction.
    /// These resources will be automatically released when the mini-transaction commits or rolls back.
    ///
    /// # Arguments
    /// * `slot` - The memo slot representing the resource to be recorded
    ///
    /// Panics in debug mode if the mini-transaction is not in the ACTIVE state
    pub fn memo_push(&mut self, slot: MemoSlot) {
        ut_ad!(
            self.mtr_state == MtrState::MtrActive,
            "memo push is only allowed in the ACTIVE state"
        );
        // ut_ad!(
        //     self.memo.contains(&slot) == false,
        //     "memo_push: slot already exists in memo"
        // );

        self.memo.push(slot);
    }

    /// Check if memo contains the given item.
    ///
    /// This function checks if the mini-transaction's memo list contains
    /// a specific resource that matches the provided slot.
    ///
    /// # Arguments
    /// * `slot` - The memo slot to check for
    ///
    /// # Returns
    /// `true` if the memo list contains the specified slot, `false` otherwise
    pub fn memo_contains(&self, slot: &MemoSlot) -> bool {
        ut_ad!(
            self.mtr_state == MtrState::MtrActive
                || self.mtr_state == MtrState::MtrCommitting
        );

        self.memo.contains(slot)
    }

    /// Release a specific memo slot while the MTR is still ACTIVE.
    ///
    /// This method is used when the caller wants to explicitly release
    /// a resource *before* the mini-transaction commits.
    ///
    /// It searches the memo stack from the top (LIFO) down to find the
    /// most recent matching slot. If found:
    ///   • it removes that slot from the memo list
    ///   • performs the appropriate cleanup action:
    ///        - PageFix  → unfix page in buffer pool
    ///        - PageDirty → (no-op; dirty pages stay dirty until flushed)
    ///        - Latch     → release shared/exclusive latch
    ///        - Other     → custom cleanup if needed
    ///
    /// ## State Requirements
    /// This function may only be called when the MTR is **ACTIVE**.
    /// A panic occurs otherwise.
    ///
    /// ## Why this exists
    /// InnoDB allows releasing individual memo entries while the
    /// MTR is still running—for example:
    ///   - early page unfix
    ///   - releasing a latch earlier than commit
    ///   - selective resource cleanup during rollback_to_savepoint()
    ///
    /// `memo_release` *removes* the entry from the memo vector entirely,
    /// unlike `memo_slot_release`, which only clears the slot but
    /// does not shrink the vector.
    ///
    /// ## Panics
    /// - If the MTR is not ACTIVE.
    /// - If the matching memo slot is not found.
    /// - If buf_pool is not attached when releasing a PageFix.
    pub fn memo_release(&mut self, to_release: &MemoSlot) {
        ut_ad!(
            self.mtr_state == MtrState::MtrActive,
            "memo_release only allowed when ACTIVE"
        );

        if let Some(pos) = self.memo.iter().rposition(|s| *s == *to_release) {
            let slot = self.memo.remove(pos);
            self.release_slot_resource(slot);
        } else {
            ut_panic!("memo_release: slot not found");
        }
    }

    /// Sets a savepoint in the mini-transaction.
    ///
    /// A savepoint marks the current state of the memo stack, allowing
    /// for partial rollbacks to this point later.
    ///
    /// # Returns
    /// The index of the newly created savepoint.
    pub fn set_savepoint(&mut self) -> Savepoint {
        let sp = self.memo.len();
        self.savepoints.push(sp);
        sp
    }
    /// Gets the current savepoint from the mini-transaction.
    ///
    /// # Returns
    /// The current savepoint index.
    ///
    /// Panics if no savepoint has been set.
    pub fn get_savepoint(&self) -> Savepoint {
        *self.savepoints.last().expect("get_savepoint: no savepoint set")
    }

    /// Gets the current log mode of the mini-transaction.
    ///
    /// # Returns
    /// The current log mode.
    pub fn get_log_mode(&self) -> MtrLogMode {
        self.log_mode
    }

    /// Sets a new log mode for the mini-transaction.
    ///
    /// # Arguments
    /// * `new_mode` - The new log mode to set
    ///
    /// # Returns
    /// The previous log mode.
    pub fn set_log_mode(&mut self, new_mode: MtrLogMode) -> MtrLogMode {
        let old = self.log_mode;
        self.log_mode = new_mode;
        old
    }

    /// Release a memo slot **in-place**—used internally by commit() and rollback.
    ///
    /// This helper performs the same logical cleanup as `memo_release()`,
    /// but with two important differences:
    ///
    /// ### 1. Slot is replaced with a dummy instead of removing it

    /// This prevents double-release during commit, because the caller
    /// (`commit()` or `rollback_to_savepoint()`) may continue iterating
    /// through the memo array after this operation.
    ///
    /// ### 2. Does NOT require the MTR to be ACTIVE
    /// Unlike `memo_release()`, this function is intentionally allowed
    /// during the **COMMITTING** phase.
    /// InnoDB’s commit logic does:
    /// ```cpp
    /// mtr->m_state = MTR_COMMITTING;
    /// mtr_memo_pop_all()  // calls mtr_memo_slot_release()
    /// ```
    /// So asserting the ACTIVE state here would break commit.
    ///
    /// ## When used
    /// - `commit()` when cleaning up *all* memo slots
    /// - `rollback_to_savepoint()`
    /// - internal selective cleanup
    ///
    /// ## What it does
    /// Based on the variant it:
    ///   • PageFix(page_id, mode) → buf_pool.release_page(page_id, mode)
    ///   • PageDirty(_) → no-op (dirty state persists)
    ///   • Latch(_, mode) → release latch (currently stubbed)
    ///   • Other(_) → no-op
    ///
    /// ## Panics
    /// - If buf_pool is not attached when releasing a PageFix.
    /// - Never checks or enforces the MTR state.
    pub fn memo_slot_release(&mut self, slot: &mut MemoSlot) {
        // Replace slot with a dummy to avoid double release
        let owned = std::mem::replace(
            slot,
            MemoSlot::Other(OtherResource { id: 0, flags: 0 }),
        );

        self.release_slot_resource(owned);
    }

    /// Commits the mini-transaction, making all its changes permanent.
    ///
    /// This method transitions the mini-transaction from the ACTIVE state to COMMITTING,
    /// flushes the log buffer to the shared redo-log, releases all remaining resources
    /// in the memo list, and finally transitions to the COMMITTED and then UNDEFINED state.
    ///
    /// # Implementation Details
    /// The commit process involves:
    /// 1. Flushing the log buffer to the shared redo-log
    /// 2. Releasing all remaining memo slots (similar to rollback_to_savepoint(0))
    /// 3. Clearing all savepoints
    /// 4. Transitioning through state changes: ACTIVE -> COMMITTING -> COMMITTED -> UNDEFINED
    ///
    pub fn commit(&mut self) {
        ut_ad!(self.mtr_state == MtrState::MtrActive);
        self.mtr_state = MtrState::MtrCommitting;
        // 1) flush self.log_buffer.buf into the shared redo‐log
        //    (call into your Log Manager API here)
        // 2) release all remaining memo slots (same as rollback_to_savepoint(0))
        while let Some(slot) = self.memo.pop() { /* ... */ }
        self.savepoints.clear();
        self.mtr_state = MtrState::MtrCommitted;
        self.magic_n = 0;
        // Optionally transition to UNDEFINED
        self.mtr_state = MtrState::MtrUndefined;
    }

    /// Mark that this MTR is doing a doublewrite-buffer create.
    /// In InnoDB, every page write goes through an MTR so we can record a redo entry.
    ///  But when we’re in the special double-write phase (copying pages into the double-write area),
    /// we don’t want to generate new redo log records for those low-level copies.
    /// So right before we start that work, we call:
    pub fn dblwr_create_in_progress(&mut self) -> &mut Self {
        self.dblwr_create_in_progress = true;
        self
    }

    /// Release all memo entries (locks, latches, etc.) acquired after the given savepoint.  
    /// Imagine your mini‐transaction as a stack of little “to-do” entries—every time you latch a page, fix a buffer, or grab any resource,
    /// you push a note onto that stack.
    /// A savepoint is just a marker you drop into the stack to remember “this spot.”
    ///
    pub fn rollback_to_savepoint(&mut self, savepoint: usize) {
        ut_ad!(
            self.magic_n == MTR_MAGIC_N,
            "MTR magic mismatch in rollback_to_savepoint"
        );
        ut_ad!(
            self.mtr_state == MtrState::MtrActive,
            "must be ACTIVE to rollback to savepoint"
        );

        // 1) Pop & release each slot beyond the savepoint
        while self.memo.len() > savepoint {
            let slot = self.memo.pop().unwrap();
            match slot {
                MemoSlot::PageFix(page_id, mode) => {
                    // e.g. buf_pool.unfix(page, mode);
                }
                MemoSlot::PageDirty(page) => {
                    // maybe clear dirty flag or no-op
                }
                MemoSlot::Latch(handle, mode) => {
                    // e.g. handle.release(mode);
                }
                MemoSlot::Other(res) => {
                    // any custom cleanup for OtherResource
                }
            }
        }

        // 2) Remove the savepoint marker itself
        if let Some(popped_sp) = self.savepoints.pop() {
            ut_ad!(
                popped_sp == savepoint,
                "rollback_to_savepoint: popped savepoint {popped_sp}, but expected {savepoint}"
            );
        } else {
            ut_panic!("rollback_to_savepoint: no savepoint to pop");
        }
    }

    /// Drain & release *all* memo slots (the C++ `mtr_memo_pop_all`).
    ///
    /// # Panics
    ///
    /// - If the magic number has been corrupted.
    /// - If we're not in the COMMITTING state.
    pub fn memo_pop_all(&mut self) {
        // debug‐only assertions to mirror the C++ ut_ad checks:
        ut_ad!(self.magic_n == MTR_MAGIC_N);
        ut_ad!(self.mtr_state == MtrState::MtrCommitting);

        // Pop off *each* MemoSlot, from last to first:
        while let Some(mut slot) = self.memo.pop() {
            // We take ownership of the slot, wrap it in a temporary `&mut`
            // so we can call your in-place release helper:
            self.memo_slot_release(&mut slot);
        }
    }

    /// Read a 1-, 2-, or 4-byte ULINT from `buf` (all big-endian),
    /// only allowed when the MTR is ACTIVE and the page has been fixed.
    pub fn read_ulint(&self, buf: &[u8], ty: usize) -> u64 {
        ut_ad!(self.is_active(), "read_ulint only in ACTIVE state");
        // Here you’d assert the page-that-owns-`buf` is in memo, e.g.:
        // ut_ad!(self.memo_contains_page(ptr, PAGE_S_FIX) || self.memo_contains_page(ptr, PAGE_X_FIX));

        ut_ad!([1, 2, 4].contains(&ty), "type must be 1,2, or 4");
        ut_ad!(buf.len() >= ty, "buffer too small for read_ulint");

        let val = match ty {
            1 => mach_read_from_1(&buf[0..1]) as u64,
            2 => mach_read_from_2(&buf[0..2]) as u64,
            4 => mach_read_from_4(&buf[0..4]) as u64,
            _ => unreachable!(),
        };
        val
    }

    /// Read an 8-byte ULINT from `buf` (big-endian).
    ///
    /// This method mirrors the C++ `mtr_t::read_uint64(ptr)` function.
    ///
    /// # Arguments
    /// * `buf` - The buffer to read from (must be at least 8 bytes)
    ///
    /// # Returns
    /// The unsigned 64-bit integer value read from the buffer
    ///
    pub fn read_uint64(&self, buf: &[u8]) -> u64 {
        ut_ad!(self.is_active(), "read_uint64 only in ACTIVE state");
        // ut_ad!(self.memo_contains_page(ptr, PAGE_S_FIX) || self.memo_contains_page(ptr, PAGE_X_FIX));

        ut_ad!(buf.len() >= 8, "buffer too small for read_uint64");

        let high = mach_read_from_4(&buf[0..4]) as u64;
        let low = mach_read_from_4(&buf[4..8]) as u64;
        (high << 32) | low
    }
    /// Increment the number of log records in the mini-transaction.
    pub fn inc_n_log_records(&mut self) {
        self.n_log_records += 1;
    }

    /// Gets the number of log records in the mini-transaction.
    ///
    /// # Returns
    /// The count of log records.
    pub fn get_n_log_records(&self) -> usize {
        self.n_log_records
    }

    /// Disable redo logging for this mini-transaction.
    ///
    /// This sets the log mode to NONE, preventing any redo log records
    /// from being generated during this mini-transaction.
    pub fn disable_redo_logging(&mut self) {
        self.log_mode = MtrLogMode::MtrLogNone;
    }

    /// Release a shared latch stored at a savepoint.
    ///
    /// # Arguments
    /// * `savepoint` - The savepoint at which the latch was acquired
    /// * `handle` - The latch handle to release
    pub fn release_s_latch_at_savepoint(
        &mut self,
        savepoint: Savepoint,
        handle: LatchHandle,
    ) {
        // TODO: check memo[savepoint] matches Latch {{ Shared }} and release it
    }

    /// Acquire a shared latch and record it in the memo structure.
    ///
    /// # Arguments
    /// * `handle` - The latch handle to acquire in shared mode
    pub fn s_lock(&mut self, handle: &mut LatchHandle) {
        // handle.lock_shared();
        self.memo_push(MemoSlot::Latch(*handle, LockMode::Shared));
    }

    /// Acquire an exclusive latch and record it in the memo structure.
    ///
    /// # Arguments
    /// * `handle` - The latch handle to acquire in exclusive mode
    pub fn x_lock(&mut self, handle: &mut LatchHandle) {
        // handle.lock_exclusive();
        self.memo_push(MemoSlot::Latch(*handle, LockMode::Exclusive));
    }

    /// Checks if this mini-transaction is in the active state.
    ///
    /// # Returns
    /// `true` if the mini-transaction is active, `false` otherwise
    pub fn is_active(&self) -> bool {
        self.mtr_state == MtrState::MtrActive
    }

    ///A precise method to unfix and unlatch exactly ONE page fix that happened after a given savepoint,
    /// without affecting the rest of the mini-transaction.
    ///
    /// It is used for:
    ///•	Partial rollback of page fixes
    /// •	Selective early release of pages
    /// •	Fine-grained B-tree operations
    /// •	Maintaining correctness during multi-step page manipulation
    ///
    /// # Arguments
    /// * `savepoint` - The savepoint at which the block was acquired
    /// * `block` - The buffer block to release
    ///
    /// # Panic
    /// Panics if the savepoint is invalid or if the slot at the savepoint is not a page slot.
    pub fn release_block_at_savepoint(
        &mut self,
        savepoint: Savepoint,
        block: &mut BufBlock,
    ) {
        ut_ad!(self.is_active(), "release only in ACTIVE state");
        #[cfg(debug_assertions)]
        {
            ut_ad!(self.magic_n == MTR_MAGIC_N, "magic mismatch");
        }
        assert!(savepoint < self.memo.len(), "invalid savepoint");

        // 1) ensure the memo slot really is that page:
        let mut slot_ref = std::mem::replace(
            &mut self.memo[savepoint],
            MemoSlot::Other(OtherResource { id: 0, flags: 0 }),
        );
        match slot_ref {
            MemoSlot::PageFix(page_id, ..) | MemoSlot::PageDirty(page_id) => {
                // In debug: check page ID matches block
                ut_ad!(block.buf_block_get_page().get_page_id() == page_id);
            }
            _ => ut_panic!("slot at {} is not a page slot", savepoint),
        }

        // 2) release that slot (unfix/dirty) in place
        self.memo_slot_release(&mut slot_ref);
    }

    /// Gets a reference to the current state of the mini-transaction.
    ///
    /// # Returns
    /// A reference to the current mini-transaction state.
    pub fn mtr_state(&self) -> &MtrState {
        &self.mtr_state
    }

    /// Gets the current log mode of the mini-transaction.
    ///
    /// # Returns
    /// The current log mode.
    pub fn log_mode(&self) -> MtrLogMode {
        self.log_mode
    }

    /// Checks if this mini-transaction has made any modifications.
    ///
    /// # Returns
    /// `true` if modifications have been made, `false` otherwise.
    pub fn modifications(&self) -> bool {
        self.modifications
    }

    /// Gets the number of log records in the mini-transaction.
    ///
    /// # Returns
    /// The count of log records.
    pub fn n_log_records(&self) -> usize {
        self.n_log_records
    }

    /// Gets a reference to the memo structure of the mini-transaction.
    ///
    /// The memo structure contains all resources acquired by this mini-transaction.
    ///
    /// # Returns
    /// A reference to the vector of memo slots.
    pub fn memo(&self) -> &Vec<MemoSlot> {
        &self.memo
    }

    /// Gets a reference to the savepoints vector of the mini-transaction.
    ///
    /// # Returns
    /// A reference to the vector of savepoints.
    pub fn savepoints(&self) -> &Vec<Savepoint> {
        &self.savepoints
    }

    /// Gets the magic number of the mini-transaction.
    ///
    /// This is used for debugging to verify the integrity of the mini-transaction.
    ///
    /// # Returns
    /// The magic number.
    pub fn magic_n(&self) -> u32 {
        self.magic_n
    }

    /// Checks if a doublewrite buffer creation is in progress.
    ///
    /// # Returns
    /// `true` if a doublewrite buffer creation is in progress, `false` otherwise.
    pub fn get_dblwr_create_in_progress(&self) -> bool {
        self.dblwr_create_in_progress
    }

    /// Reserves space in the log buffer and writes the mini-transaction's log records to it.
    ///
    /// This method attempts to write the mini-transaction's log buffer to the shared redo log.
    /// It first tries a fast path to fit the data in the current log block. If that fails,
    /// it falls back to a more comprehensive write operation.
    ///
    /// # Arguments
    /// * `recovery` - A recovery flag used by the log system
    /// * `log` - A thread-safe reference to the log system
    ///
    /// # Returns
    /// * `Ok(())` if the write operation was successful
    /// * `Err(String)` with an error message if the write operation failed
    ///
    pub fn log_reserve_and_write(
        &mut self,
        recovery: u32,
        log: Arc<Mutex<Log>>,
    ) -> Result<(), String> {
        let data = &self.log_buffer.buf;

        // Fast‐path: try to fit in the current block.
        if let Ok(res) = log.lock().reserve_and_write_fast(data) {
            // self.m_end_lsn = res.end_lsn;
            return Ok(());
        }

        // Fallback: write it all via write_low, which does the open/reserve internally.
        let res = log
            .lock()
            .write_low(data)
            .map_err(|e| format!("write_low failed: {}", e))?;
        // self.m_end_lsn = res.end_lsn;

        // Finally close/flush the log if your Log API requires it.
        /*   self.m_end_lsn = log.close(recovery)
        .map_err(|e| format!("close failed: {}", e))?;*/

        Ok(())
    }

    // mlog calling methods
    /// Writes a 32-bit unsigned integer to the log buffer.
    ///
    /// # Arguments
    /// * `page` - The page being modified
    /// * `offset` - The offset within the page
    /// * `val` - The 32-bit value to write
    /// * `mlog_type` - The type of log record to create
    pub fn write_u32(
        &mut self,
        page: &Page,
        offset: usize,
        val: u32,
        mlog_type: MlogType,
    ) {
        self.log_buffer.mlog_write_u32(
            page,
            offset,
            val,
            mlog_type,
            self.log_mode(),
        );
        self.inc_n_log_records();
    }

    /// Writes a 64-bit unsigned integer to the log buffer.
    ///
    /// # Arguments
    /// * `page` - The page being modified
    /// * `offset` - The offset within the page
    /// * `val` - The 64-bit value to write
    pub fn write_uint64(&mut self, page: &Page, offset: usize, val: u64) {
        self.log_buffer.mlog_write_uint64(page, offset, val, self.log_mode());
        self.inc_n_log_records();
    }

    /// Concatenates a 64-bit unsigned integer in a compressed format to the log buffer.
    ///
    /// This method appends a compressed representation of a 64-bit integer to the
    /// mini-transaction's log buffer. The compression reduces the space required
    /// for storing large integers in the log, especially when many high-order bits are zero.
    ///
    /// # Arguments
    /// * `val` - The 64-bit unsigned integer to be compressed and appended to the log buffer
    ///
    /// # Implementation Details
    /// The compression is performed by the underlying log buffer's `mlog_catenate_u64_compressed` method,
    /// which uses the current log mode to determine whether logging is enabled.
    pub fn catenate_u64_compressed(&mut self, val: u64) {
        self.log_buffer.mlog_catenate_u64_compressed(val, self.log_mode)
    }

    /// Writes a byte string to the log buffer.
    ///
    /// # Arguments
    /// * `page` - The page being modified
    /// * `offset` - The offset within the page
    /// * `data` - The byte slice to write
    pub fn write_string(&mut self, page: &Page, offset: usize, data: &[u8]) {
        self.log_buffer.mlog_write_string(page, offset, data, self.log_mode());
        self.inc_n_log_records();
    }

    /// Catenate string to the log buffer.
    ///
    /// # Arguments
    ///
    /// * `page` - The page to copy contents from.
    /// * `offset` - Offset in the page to copy from.
    /// * `len` - Length of bytes to copy.
    ///
    pub fn catenate_string(&mut self, page: &Page, offset: usize, len: usize) {
        let page_frame = page.to_page_frame();
        self.log_buffer.mlog_catenate_string(
            &page_frame.0[offset..],
            offset,
            len,
            self.log_mode,
        );
    }

    /// Reserves space and writes an index payload to the log buffer.
    ///
    /// This method allocates space in the log buffer for an index-related
    /// operation and returns a mutable slice for the caller to fill with data.
    ///
    /// # Arguments
    /// * `page` - The page being modified
    /// * `mlog_type` - The type of log record to create
    /// * `size` - The size of the payload to reserve
    ///
    /// # Returns
    /// An optional mutable slice to the reserved payload area, or None if logging is disabled
    pub fn open_and_write_index(
        &mut self,
        page: &Page,
        mlog_type: MlogType,
        size: usize,
    ) -> Option<&mut [u8]> {
        let payload = self.log_buffer.mlog_open_and_write_index(
            page,
            mlog_type,
            size,
            self.log_mode(),
        );

        if payload.is_some() {
            self.n_log_records += 1;
        }

        payload
    }

    /// Writes the initial part of a log record consisting of a one-byte type and four-byte space id and page number.
    ///
    /// This method writes the header portion of a log record to the mini-transaction's log buffer.
    /// The header includes the log record type, space ID, and page number, which are essential
    /// for identifying the affected page during recovery.
    ///
    /// # Arguments
    /// * `page` - The page being modified, which provides the space ID and page number
    /// * `mlog_type` - The type of log record to create
    /// * `log_mode` - The logging mode that determines whether logging is enabled
    ///
    /// # Implementation Details
    /// The method delegates to the log buffer's `mlog_write_initial_log_record` method,
    /// passing the page, log type, log mode, and a flag indicating this is not a multi-record operation.
    pub fn write_initial_log_record(
        &mut self,
        page: &Page,
        mlog_type: MlogType,
        log_mode: MtrLogMode,
    ) {
        self.log_buffer
            .mlog_write_initial_log_record(page, mlog_type, log_mode, false)
    }

    /// Marks the end of a multi-record log entry.
    ///
    /// This method is called to finalize a sequence of related log records
    /// that should be treated as a single atomic operation.
    pub fn multi_record_end(&mut self) {
        self.log_buffer.mlog_multi_rec_end();
        self.inc_n_log_records();
    }

    pub fn get_mtr_state(&self) -> MtrState {
        self.mtr_state.clone()
    }

    pub fn set_mtr_state(&mut self, mtr_state: MtrState) {
        self.mtr_state = mtr_state;
    }

    pub fn attach_buf_pool<P>(&mut self, pool: Arc<P>)
    where
        P: BufPoolLike + 'static,
    {
        let trait_object: Arc<dyn BufPoolLike> = pool;
        self.buf_pool = Some(trait_object);
    }
    /// Internal helper: releases a MemoSlot's resource.
    /// Returns nothing — caller decides whether to remove slot or replace with dummy.
    fn release_slot_resource(&mut self, slot: MemoSlot) {
        match slot {
            MemoSlot::PageFix(page_id, fix_mode) => {
                if let Some(pool) = &self.buf_pool {
                    pool.release_page(page_id, fix_mode);
                } else {
                    panic!("MTR has no buf_pool attached");
                }
            }

            MemoSlot::PageDirty(_pid) => {
                // No-op — dirty pages remain dirty
            }

            MemoSlot::Latch(_handle, _mode) => {
                // No-op in your minimal implementation
            }

            MemoSlot::Other(_) => {
                // No-op
            }
        }
    }
}
