pub type UndoNo = u64;
pub enum TrxStatus {
    TrxNotStarted,
    TrxActive,
    TrxCommittedInMemory,
    TrxPrepared,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrxQueState {
    /// The Transaction is running.
    TrxQueRunning,
    /// The Transaction is waiting for a lock.
    TrxQueLockWait,
    /// The Transaction is rolling back.
    TrxQueRollingBack,
    /// The Transaction is committing.
    TrxQueCommitting,
}
