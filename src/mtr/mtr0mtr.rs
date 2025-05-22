pub(crate) use crate::mtr::mtr0types::MtrState;

pub struct Mtr {
    n_log_records: usize,
    mtr_state: MtrState,
}

impl Mtr {
    pub fn new() -> Self {
        Self { n_log_records: 0, mtr_state: MtrState::MTR_UNDEFINED }
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
}
