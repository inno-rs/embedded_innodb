pub(crate) use crate::mtr::mtr0types::{
    LatchHandle, LockMode, MemoSlot, MtrLogMode, MtrState, Savepoint,
};
use crate::{
    buf::buf0types::BufBlock,
    mtr::{
        mtr0log::MLog,
        mtr0types::{MlogType, OtherResource},
    },
    page::page0page::Page,
    ut::ut0dbg::ut_ad,
    ut_panic,
};

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
}

impl Mtr {
    /// Creates a new mini-transaction in an undefined state.
    ///
    /// # Returns
    /// A new Mtr instance with default values and state set to undefined.
    pub fn new() -> Self {
        Self {
            mtr_state: MtrState::MtrUndefined,
            log_mode: MtrLogMode::MTR_LOG_NONE,
            modifications: false,
            n_log_records: 0,
            memo: Vec::new(),
            log_buffer: MLog::new(),
            savepoints: Vec::new(),
            dblwr_create_in_progress: false,
            magic_n: MTR_MAGIC_N,
        }
    }

    /// Starts a new mini-transaction by setting its state to ACTIVE
    /// and resetting the log record count.
    /// Start (or restart) this mini‐transaction.
    pub fn start(&mut self) -> &mut Self {
        // logging mode → ALL
        self.log_mode = MtrLogMode::MTR_LOG_MODE_ALL;
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

        self
    }

    ///  Record a resource so it can be undone later.
    pub fn memo_push(&mut self, slot: MemoSlot) {
        ut_ad!(
            self.mtr_state == MtrState::MtrActive,
            "memo push is only allowed in the ACTIVE state"
        );
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

        self.memo.iter().any(|&x| x == *slot)
    }

    /// Returns a mutable reference to the mini-transaction's log buffer.
    ///
    /// This method provides access to the log buffer, allowing operations
    /// to be recorded in the mini-transaction log.
    ///
    /// # Returns
    /// A mutable reference to the MLog buffer
    pub fn mlog(&mut self) -> &mut MLog {
        &mut self.log_buffer
    }

    pub fn memo_release(&mut self, to_release: &MemoSlot) {
        ut_ad!(
            self.mtr_state == MtrState::MtrActive,
            "memo release only allowed when ACTIVE"
        );
        if let Some(pos) = self.memo.iter().rposition(|s| s == to_release) {
            let slot = self.memo.remove(pos);
            // release slot exactly as in rollback_to_savepoint:
            match slot {
                MemoSlot::PageFix(page, mode) => {
                    /* buf_pool.unfix(page, mode) */
                }
                MemoSlot::PageDirty(page) => { /* clear dirty flag? */ }
                MemoSlot::Latch(handle, mode) => { /* handle.release(mode) */ }
                MemoSlot::Other(res) => { /* custom cleanup */ }
            }
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
    /// # Panics
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

    /// Release a single memo slot in-place (C++ `mtr_memo_slot_release`).
    pub fn memo_slot_release(&mut self, slot: &mut MemoSlot) {
        // Swap out the slot with a dummy so we don't double-release
        let owned = std::mem::replace(
            slot,
            MemoSlot::Other(OtherResource { id: 0, flags: 0 }),
        );
        match owned {
            MemoSlot::PageFix(page_id, mode) => {
                // TODO: call buffer-pool release: srv_buf_pool.release(page, mode, self)
            }
            MemoSlot::PageDirty(page) => {
                // TODO: release dirty page: srv_buf_pool.release(page, /* buf_fix type */, self)
            }
            MemoSlot::Latch(handle, mode) => {
                match mode {
                    LockMode::Shared => {
                        // TODO: rw_lock_s_unlock(handle)
                    }
                    LockMode::Exclusive => {
                        // TODO: rw_lock_x_unlock(handle)
                    }
                }
            }
            MemoSlot::Other(res) => {
                // TODO: debug assert for MTR_MEMO_MODIFY and memo_contains
            }
        }
    }

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
        #[cfg(debug_assertions)]
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
            debug_assert_eq!(
                popped_sp, savepoint,
                "rollback_to_savepoint: popped savepoint {popped_sp}, but expected {savepoint}"
            );
        } else {
            ut_panic!("rollback_to_savepoint: no savepoint to pop");
        }
    }

    /// Read a variable‐length “ulint” (1–8 bytes, little‐endian) from `buf[0..type]`. .
    /// Reads a variable-length unsigned integer (1-8 bytes, little-endian) from a buffer.
    ///
    /// # Arguments
    /// * `buf` - The buffer to read from
    /// * `ty` - The number of bytes to read (1-8)
    ///
    /// # Returns
    /// The unsigned 64-bit integer value read from the buffer
    ///
    /// # Panics
    /// Panics if `ty` is greater than 8 or if the buffer is too small
    pub fn read_ulint(&self, buf: &[u8], ty: usize) -> u64 {
        ut_ad!(ty <= 8, "type must be 1..=8");
        ut_ad!(buf.len() >= ty, "buffer too small");

        // Copy the first `ty` bytes into a zeroed 8-byte array,
        // then convert that full array from Little Endian → u64.
        let mut tmp = [0u8; 8];
        tmp[..ty].copy_from_slice(&buf[..ty]);
        u64::from_le_bytes(tmp)
    }

    /// Reads an 8-byte little-endian unsigned integer from a buffer.
    ///
    /// This method mirrors the C++ `mtr_t::read_uint64(ptr)` function.
    ///
    /// # Arguments
    /// * `buf` - The buffer to read from (must be at least 8 bytes)
    ///
    /// # Returns
    /// The unsigned 64-bit integer value read from the buffer
    ///
    /// # Panics
    /// Panics if the buffer is less than 8 bytes
    pub fn read_uint64(&self, buf: &[u8]) -> u64 {
        let bytes: [u8; 8] =
            buf[0..8].try_into().expect("buffer too small for read_uint64");
        u64::from_le_bytes(bytes)
    }

    /// Increments the count of log records in the mini-transaction.
    ///
    /// This is called internally whenever a new log record is added.
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

    /// Gets the current state of the mini-transaction.
    ///
    /// # Returns
    /// A clone of the current mini-transaction state.
    pub fn get_mtr_state(&self) -> MtrState {
        self.mtr_state.clone()
    }

    /// Sets the state of the mini-transaction.
    ///
    /// # Arguments
    /// * `mtr_state` - The new state to set
    pub fn set_mtr_state(&mut self, mtr_state: MtrState) {
        self.mtr_state = mtr_state;
    }

    /// Disable redo logging for this mini-transaction.
    ///
    /// This sets the log mode to NONE, preventing any redo log records
    /// from being generated during this mini-transaction.
    pub fn disable_redo_logging(&mut self) {
        self.log_mode = MtrLogMode::MTR_LOG_NONE;
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

    /// Releases a buffer block at a specific savepoint without rolling back other operations.
    ///
    /// This method allows for selective release of a buffer block that was acquired
    /// at a specific savepoint, without affecting other resources acquired by the
    /// mini-transaction.
    ///
    /// # Arguments
    /// * `savepoint` - The savepoint at which the block was acquired
    /// * `block` - The buffer block to release
    ///
    /// # Panics
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

        // 3) drop the fix count under lock
        block.fix_dec(); // Unpinning the page
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

    /// Gets a reference to the log buffer of the mini-transaction.
    ///
    /// # Returns
    /// A reference to the log buffer.
    pub fn log_buffer(&self) -> &MLog {
        &self.log_buffer
    }

    /// Gets a mutable reference to the log buffer of the mini-transaction.
    ///
    /// # Returns
    /// A mutable reference to the log buffer.
    pub fn log_buffer_mut(&mut self) -> &mut MLog {
        &mut self.log_buffer
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

    /// Marks the end of a multi-record log entry.
    ///
    /// This method is called to finalize a sequence of related log records
    /// that should be treated as a single atomic operation.
    pub fn multi_record_end(&mut self) {
        self.log_buffer.mlog_multi_rec_end();
        self.inc_n_log_records();
    }
}
