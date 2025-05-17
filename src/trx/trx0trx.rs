use crate::innodb::innodb0types::TxnId;

pub struct Trx {
    id: TxnId,
}

impl Trx {
    pub fn new(id: TxnId) -> Self {
        Trx { id }
    }

    pub fn trx_get_id(&self) -> TxnId {
        self.id
    }
}
