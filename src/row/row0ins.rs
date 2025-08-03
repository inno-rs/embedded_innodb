use std::sync::{Arc, Mutex};

use crate::{
    data::data0types::DTuple,
    dict::{dict0dict::Dict, dict0types::Index},
    innodb::innodb0types::{DbErr, TrxId},
    lock::lock0lock::LockSys,
    que::que0types::QueThr,
    ut::ut0dbg::{ut_a, ut_ad},
};

pub enum IbInsertMode {
    /// INSERT INTO ... SELECT ...
    InsSearched,
    /// INSERT INTO ... VALUES ...
    InsValues,
    /// Insert directly into a table.
    InsDirect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IbInsertState {
    /// Set IX lock on the table.
    InsNodeSetIXLock,
    /// Allocate a row ID for the new record.
    InsNodeAllocRowId,
    /// Index entries should be built and inserted.
    InsNodeIndexEntries,
}

struct InsertNode {
    /// The mode of the insert operation.
    ins_type: IbInsertMode,
    /// The state of the insert operation.
    ins_state: IbInsertState,
    /// The index to insert into.
    index: Arc<Index>,
    /// The list of entries to insert. One for each index.
    entries: Vec<DTuple>,
    /// Transaction id of the last transaction that executed this node.
    trx_id: TrxId,
}

pub struct RowInsert {
    /// Reference to data dictionary.
    dict: Arc<Dict>,
    /// Reference to the lock system.
    lock_sys: Arc<Mutex<LockSys>>,
}

impl RowInsert {
    pub fn new(dict: Arc<Dict>, lock_sys: Arc<Mutex<LockSys>>) -> Self {
        Self { dict, lock_sys }
    }

    /// Try to insert an index entry (record) in the btree index.
    ///
    pub fn index_entry_low(
        &self,
        mode: usize,
        index: Arc<Index>,
        dtuple: &DTuple,
        n_ext: usize,
    ) -> Result<(), DbErr> {
        Ok(())
    }

    pub fn index(
        &self,
        insert_node: InsertNode,
        que_thr: Arc<QueThr>,
    ) -> Result<(), DbErr> {
        Ok(())
    }

    pub fn alloc_row_id_step(&self, insert_node: &mut InsertNode) {
        ut_ad!(insert_node.ins_state == IbInsertState::InsNodeAllocRowId);

        if insert_node.index.is_unique() == false {}
    }
}
