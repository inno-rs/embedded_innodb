//! Lightweight Key-Value API built on top of the internal B-Tree layer.
//!
//! This module exposes a thin facade that lets tests and examples interact with the
//! storage engine using simple key-value semantics.
//! Internally, it routes operations
//! to the B-Tree access methods, constructing dtuples and walking cursors as needed.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    btr::{btr0btr::BTree, btr0cur::BTreeCursor},
    buf::buf0buf::BufPool,
    dict::dict0types::Index,
    lock::lock0lock::LockSys,
    mtr::mtr0mtr::Mtr,
    page::page0cur::PAGE_CUR_LE,
    rem::rem0rec::RecordFieldData,
    some_or_panic,
    tests::test0utils::{
        FieldType, create_dtuple, create_dtuples_from_input_data_chunk,
    },
};

/// Simple key-value wrapper around the B-Tree.
/// Intended mainly for perf tests.
///
/// The store keeps references to the core engine components required to
/// perform B-Tree operations:
/// - btree: the physical B-Tree structure
/// - index: logical index metadata (schema)
/// - buf_pool: buffer pool for page access
/// - lock_sys: lock system for concurrency control
pub struct KeyValueStore {
    btree: Arc<BTree>,
    index: Arc<Index>,
    buf_pool: Arc<BufPool>,
    lock_sys: Arc<Mutex<LockSys>>,
}

/// Result of a key-value operation.
///
/// This is a minimal status indicator used by the facade methods.
pub enum Status {
    /// Operation completed successfully.
    Ok,
    /// Operation failed. Specific errors are not yet surfaced here.
    Error,
}

impl KeyValueStore {
    /// Construct a new key-value store facade.
    ///
    /// # Arguments
    ///
    /// - btree: Shared reference to the underlying B-Tree structure.
    /// - index: The logical index describing the tuple layout.
    /// - buf_pool: Buffer pool used to access and modify pages.
    /// - lock_sys: Lock system used for concurrency control.
    pub fn new(
        btree: Arc<BTree>,
        index: Arc<Index>,
        buf_pool: Arc<BufPool>,
        lock_sys: Arc<Mutex<LockSys>>,
    ) -> Self {
        Self { btree, index, buf_pool, lock_sys }
    }

    /// Read a single record by its key and return its field values.
    ///
    /// # Arguments
    ///
    /// * `table` - Logical table name (currently unused by the underlying engine).
    /// * `key` - The lookup key.
    /// * `values` - Output map that will be populated with field_N -> value pairs.
    ///
    /// Returns Ok if a record could be positioned and fields extracted.
    pub fn read(
        &self,
        table: String,
        key: String,
        values: &mut HashMap<String, String>,
    ) -> Status {
        let mut btree_cursor = BTreeCursor::new(
            self.btree.clone(),
            self.index.clone(),
            self.buf_pool.clone(),
            self.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

        let dtuple = create_dtuple(&[FieldType::String(Some(key.clone()))]);

        btree_cursor.search_to_nth_level(0, &dtuple, PAGE_CUR_LE, 0, &mut mtr);

        let rec = some_or_panic!(btree_cursor.get_rec());
        for i in 0..rec.rec_get_n_fields() {
            let data = rec.rec_get_data(i);
            let data = some_or_panic!(data);
            let value = match data {
                RecordFieldData::Data(data) => data,
                RecordFieldData::SqlNullData() => "NULL".to_string(),
            };

            values.insert(format!("field_{}", i), value);
        }

        Status::Ok
    }

    /// Perform a forward range scan starting from start_key and fill the provided
    /// output slice with up to count rows.
    ///
    /// # Arguments
    ///
    /// * `table` - Logical table name (currently unused by the underlying engine).
    /// * `start_key` - The inclusive starting key for the scan.
    /// * `count` - Maximum number of rows to read.
    /// * `fields` - Names of fields to project (currently unused; all fields returned).
    /// * `values` - Output slice of maps, each map represents one row.
    ///
    /// Note: This is currently a stub and returns Ok without populating results.
    pub fn scan(
        &self,
        table: String,
        start_key: String,
        count: usize,
        fields: Vec<String>,
        values: &mut [HashMap<String, String>],
    ) -> Status {
        Status::Ok
    }

    /// Update an existing record identified by key with the provided values.
    ///
    /// # Arguments
    ///
    /// * `table` - Logical table name (currently unused by the underlying engine).
    /// * `key` - The key of the record to update.
    /// * `values` - Map of field name to new value.
    ///
    /// Note: This is currently a stub and returns Ok without modifying storage.
    pub fn update(
        &self,
        table: String,
        key: String,
        values: HashMap<String, String>,
    ) -> Status {
        Status::Ok
    }

    /// Insert a new record.
    ///
    /// # Arguments
    ///
    /// * `table` - Logical table name (currently unused by the underlying engine).
    /// * `key` - The key for the new record.
    /// * `values` - Map of field name to value for the record.
    ///
    /// Returns Ok after the tuple(s) are inserted via the B-Tree cursor.
    pub fn insert(
        &self,
        table: String,
        key: String,
        values: HashMap<String, String>,
    ) -> Status {
        let mut btree_cursor = BTreeCursor::new(
            self.btree.clone(),
            self.index.clone(),
            self.buf_pool.clone(),
            self.lock_sys.clone(),
        );

        let mut mtr = Mtr::new();
        mtr.start();

        let mut input: Vec<[FieldType; 1]> = vec![];
        for (key, value) in values {
            input.push([FieldType::String(Some(value))]);
        }

        let mut dtuples = create_dtuples_from_input_data_chunk(input);
        for dtuple in dtuples.iter_mut() {
            btree_cursor.pessimistic_insert(0, dtuple, 0, &mut mtr).unwrap()
        }

        Status::Ok
    }

    /// Delete a record identified by the key.
    ///
    /// # Arguments
    ///
    /// * `table` - Logical table name (currently unused by the underlying engine).
    /// * `key` - The key of the record to delete.
    ///
    /// Note: This is currently a stub and returns Ok without modifying storage.
    pub fn delete(&self, table: String, key: String) -> Status {
        Status::Ok
    }
}
