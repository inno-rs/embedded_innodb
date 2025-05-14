pub struct Mtr {
    n_log_records: usize,
}

impl Mtr {
    pub fn new() -> Self {
        Self { n_log_records: 0 }
    }

    /// Increment the number of log records in the mini-transaction.
    pub fn inc_n_log_records(&mut self) {
        self.n_log_records += 1;
    }

    /// Get the number of log records in the mini-transaction.
    pub fn get_n_log_records(&self) -> usize {
        self.n_log_records
    }
}
