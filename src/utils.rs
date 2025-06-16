/// Unique identifier for a transaction within InnoDB.
///
/// `TxnId` is used throughout the storage engine to track the
/// lifetime of a transaction. It corresponds to the `max_trx_id`
/// field found in various on-disk structures.
pub type TxnId = i64;
