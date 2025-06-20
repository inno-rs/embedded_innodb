use std::sync::{Arc, Mutex};

use crate::{
    data::data0types::DTuple,
    dict::{dict0dict::Dict, dict0types::Index},
    innodb::innodb0types::DbErr,
    lock::lock0lock::LockSys,
};

pub struct RowInsert {
    /// Reference to data dictionary.
    dict: Arc<Mutex<Dict>>,
    /// Reference to the lock system.
    lock_sys: Arc<Mutex<LockSys>>,
}

impl RowInsert {
    pub fn new(dict: Arc<Mutex<Dict>>, lock_sys: Arc<Mutex<LockSys>>) -> Self {
        Self { dict, lock_sys }
    }

    /// Try to insert an index entry (record) in the btree index.
    ///
    pub fn index_entry_low(
        &self,
        mode: usize,
        index: Arc<Mutex<Index>>,
        dtuple: &DTuple,
        n_ext: usize,
    ) -> Result<(), DbErr> {
        let index = index.lock().unwrap();
        let btree = { self.dict.lock().unwrap().get_btree() };

        Ok(())
    }
}
