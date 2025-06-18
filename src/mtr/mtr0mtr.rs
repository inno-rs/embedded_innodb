use crate::buf::buf0types::BufBlock;
pub(crate) use crate::mtr::mtr0log::MLog;
use crate::mtr::mtr0types::OtherResource;
pub(crate) use crate::mtr::mtr0types::{
    LatchHandle, LockMode, MemoSlot, MtrLogMode, MtrState, Savepoint,
};
use crate::ut::ut0dbg::ut_ad;
use crate::ut_panic;

pub const MTR_MAGIC_N: u32 = 54551;
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
    pub fn new() -> Self {
        Self {
            mtr_state: MtrState::MTR_UNDEFINED,
            log_mode: MtrLogMode::MTR_LOG_MODE_ALL,
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
        self.mtr_state = MtrState::MTR_ACTIVE;

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
            self.mtr_state == MtrState::MTR_ACTIVE,
            "memo push is only allowed in the ACTIVE state"
        );
        self.memo.push(slot);
    }

    pub fn memo_release(&mut self, to_release: &MemoSlot) {
        ut_ad!(
            self.mtr_state == MtrState::MTR_ACTIVE,
            "memo release only allowed when ACTIVE"
        );
        if let Some(pos) = self.memo.iter().rposition(|s| s == to_release) {
            let slot = self.memo.remove(pos);
            // release slot exactly as in rollback_to_savepoint:
            match slot {
                MemoSlot::PageFix { page, mode } => {
                    /* buf_pool.unfix(page, mode) */
                }
                MemoSlot::PageDirty(page) => { /* clear dirty flag? */ }
                MemoSlot::Latch { handle, mode } => {
                    /* handle.release(mode) */
                }
                MemoSlot::Other(res) => { /* custom cleanup */ }
            }
        } else {
            ut_panic!("memo_release: slot not found");
        }
    }

    pub fn set_savepoint(&mut self) -> Savepoint {
        let sp = self.memo.len();
        self.savepoints.push(sp);
        sp
    }
    pub fn get_savepoint(&self) -> Savepoint {
        *self.savepoints.last().expect("get_savepoint: no savepoint set")
    }

    pub fn get_log_mode(&self) -> MtrLogMode {
        self.log_mode
    }

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
            MemoSlot::PageFix { page, mode } => {
                // TODO: call buffer-pool release: srv_buf_pool.release(page, mode, self)
            }
            MemoSlot::PageDirty(page) => {
                // TODO: release dirty page: srv_buf_pool.release(page, /* buf_fix type */, self)
            }
            MemoSlot::Latch { handle, mode } => {
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
        ut_ad!(self.mtr_state == MtrState::MTR_ACTIVE);
        self.mtr_state = MtrState::MTR_COMMITTING;
        // 1) flush self.log_buffer.buf into the shared redo‐log
        //    (call into your Log Manager API here)
        // 2) release all remaining memo slots (same as rollback_to_savepoint(0))
        while let Some(slot) = self.memo.pop() { /* ... */ }
        self.savepoints.clear();
        self.mtr_state = MtrState::MTR_COMMITTED;
        self.magic_n = 0;
        // Optionally transition to UNDEFINED
        self.mtr_state = MtrState::MTR_UNDEFINED;
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
            self.mtr_state == MtrState::MTR_ACTIVE,
            "must be ACTIVE to rollback to savepoint"
        );

        // 1) Pop & release each slot beyond the savepoint
        while self.memo.len() > savepoint {
            let slot = self.memo.pop().unwrap();
            match slot {
                MemoSlot::PageFix { page, mode } => {
                    // e.g. buf_pool.unfix(page, mode);
                }
                MemoSlot::PageDirty(page) => {
                    // maybe clear dirty flag or no-op
                }
                MemoSlot::Latch { handle, mode } => {
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
    pub fn read_ulint(&self, buf: &[u8], ty: usize) -> u64 {
        ut_ad!(ty <= 8, "type must be 1..=8");
        ut_ad!(buf.len() >= ty, "buffer too small");

        // Copy the first `ty` bytes into a zeroed 8-byte array,
        // then convert that full array from Little Endian → u64.
        let mut tmp = [0u8; 8];
        tmp[..ty].copy_from_slice(&buf[..ty]);
        u64::from_le_bytes(tmp)
    }

    /// Read an 8‐byte little‐endian unsigned integer.
    /// Mirrors C++ `mtr_t::read_uint64(ptr)`.
    pub fn read_uint64(&self, buf: &[u8]) -> u64 {
        let bytes: [u8; 8] =
            buf[0..8].try_into().expect("buffer too small for read_uint64");
        u64::from_le_bytes(bytes)
    }

    /// Increment the number of log records in the mini-transaction.
    pub fn inc_n_log_records(&mut self) {
        self.n_log_records += 1;
    }

    /// Get the number of log records in the mini-transaction.
    pub fn get_n_log_records(&self) -> usize {
        self.n_log_records
    }

    pub fn get_mtr_state(&self) -> MtrState {
        self.mtr_state.clone()
    }

    pub fn set_mtr_state(&mut self, mtr_state: MtrState) {
        self.mtr_state = mtr_state;
    }

    /// Disable redo logging for this mini-transaction.
    pub fn disable_redo_logging(&mut self) {
        self.log_mode = MtrLogMode::MTR_LOG_NONE;
    }

    /// Release an s-latch stored at a savepoint.
    pub fn release_s_latch_at_savepoint(
        &mut self,
        savepoint: Savepoint,
        handle: LatchHandle,
    ) {
        // TODO: check memo[savepoint] matches Latch {{ Shared }} and release it
    }

    /// Acquire a shared latch and record it.
    pub fn s_lock(&mut self, handle: &mut LatchHandle) {
        // handle.lock_shared();
        self.memo_push(MemoSlot::Latch {
            handle: *handle,
            mode: LockMode::Shared,
        });
    }

    /// Acquire an exclusive latch and record it.
    pub fn x_lock(&mut self, handle: &mut LatchHandle) {
        // handle.lock_exclusive();
        self.memo_push(MemoSlot::Latch {
            handle: *handle,
            mode: LockMode::Exclusive,
        });
    }

    /// Is this mini-transaction active?
    pub fn is_active(&self) -> bool {
        self.mtr_state == MtrState::MTR_ACTIVE
    }

    /// Release a buffer block at a savepoint without rolling back others.
    pub fn release_block_at_savepoint(
        &mut self,
        savepoint: Savepoint,
        block: &BufBlock,
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
            MemoSlot::PageFix { page, .. } | MemoSlot::PageDirty(page) => {
                // In debug: check page ID matches block
                #[cfg(debug_assertions)]
                {
                    let page_id =
                        block.buf_page.borrow().borrow().get_page_id();
                    ut_ad!(page_id == page);
                }
            }
            _ => ut_panic!("slot at {} is not a page slot", savepoint),
        }

        // 2) release that slot (unfix/dirty) in place
        self.memo_slot_release(&mut slot_ref);

        // 3) drop the fix count under lock
        block.fix_dec(); // Unpinning the page
    }

    pub fn mtr_state(&self) -> &MtrState {
        &self.mtr_state
    }

    pub fn log_mode(&self) -> MtrLogMode {
        self.log_mode
    }

    pub fn modifications(&self) -> bool {
        self.modifications
    }

    pub fn n_log_records(&self) -> usize {
        self.n_log_records
    }

    pub fn memo(&self) -> &Vec<MemoSlot> {
        &self.memo
    }

    pub fn log_buffer(&self) -> &MLog {
        &self.log_buffer
    }

    pub fn log_buffer_mut(&mut self) -> &mut MLog {
        &mut self.log_buffer
    }

    pub fn savepoints(&self) -> &Vec<Savepoint> {
        &self.savepoints
    }

    pub fn magic_n(&self) -> u32 {
        self.magic_n
    }
}
