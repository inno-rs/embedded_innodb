use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    const_assert,
    data::data0types::DATA_N_SYS_COLS,
    dict::{
        dict0table::{Table, TableId},
        dict0types::{Index, IndexId},
    },
    fsp::fsp0fsp::Fsp,
    ut::ut0dbg::ut_ad,
};

/// The size of a big row in InnoDB.
const BIG_ROW_SIZE: usize = 1024;

/// The `Dict` struct represents a dictionary in the InnoDB storage engine.
/// It is responsible for managing the metadata of tables and indexes.
pub struct Dict {
    /// Reference to Fsp.
    fsp: Arc<Mutex<Fsp>>,
    /// Inner dictionary structure containing tables.
    inner: Arc<Mutex<DictInner>>,
}

/// Inner structure for the dictionary that holds the tables.
/// This structure is wrapped in a Mutex to allow safe concurrent access.
/// It contains two hashmaps:
/// - `tables`: Maps table names to their corresponding `Arc<Table>`.
/// - `tables_by_id`: Maps table IDs to their corresponding `Arc<Table>`.
///   This allows for efficient retrieval of tables by both name and ID.
///
pub struct DictInner {
    /// Hashmap of tables based on table name.
    tables: HashMap<String, Arc<Table>>,
    /// Hashmap of tables based on table id.
    tables_by_id: HashMap<TableId, Arc<Table>>,
}

/// Implementation of the Dict struct, which provides methods to manage the dictionary of tables.
/// This includes adding tables to the cache, retrieving tables by name or ID,
/// and checking if a column name is reserved in InnoDB.
///
impl Dict {
    pub fn new(fsp: Arc<Mutex<Fsp>>) -> Self {
        Self {
            fsp,
            inner: Arc::new(Mutex::new(DictInner {
                tables: Default::default(),
                tables_by_id: Default::default(),
            })),
        }
    }

    pub fn get_fsp(&self) -> Arc<Mutex<Fsp>> {
        self.fsp.clone()
    }

    /// Determines if a table name is reserved in InnoDB.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the table to check.
    ///
    /// /// # Returns
    ///
    /// A boolean indicating whether the table name is reserved.
    ///
    pub fn col_name_is_reserved(name: &str) -> bool {
        // Check if the column name is reserved in InnoDB.
        const_assert!(DATA_N_SYS_COLS == 3, "error DATA_N_SYS_COLS must be 3");
        matches!(name, "DB_ROW_ID" | "DB_TRX_ID" | "DB_ROLL_PTR")
    }

    /// Adds a table to the cache.
    ///
    /// # Arguments
    ///
    /// * `table` - An `Arc<Table>` representing the table to add.
    ///
    pub fn table_add_to_cache(&self, table: Arc<Table>) {
        let mut inner = self.inner.lock().unwrap();
        let table_name = table.get_name().to_string();
        let table_id = table.get_id();

        let contains = inner.tables.contains_key(&table_name);
        ut_ad!(!contains, "Table {} already exists in cache", table_name);

        let contains = inner.tables_by_id.contains_key(&table_id);
        ut_ad!(
            !contains,
            "Table with ID {} already exists in cache",
            table_id
        );

        inner.tables.insert(table_name, table.clone());
        inner.tables_by_id.insert(table_id, table.clone());

        table.set_cached();

        let row_len = table
            .get_cols()
            .iter()
            .map(|col| col.get_max_size())
            .sum::<usize>();

        if row_len > BIG_ROW_SIZE {
            table.set_big_rows();
        }
    }

    /// Retrieves a table by its ID from the cache.
    ///
    /// # Arguments
    ///
    /// * `table_id` - The ID of the table to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<Arc<Table>>` containing the table if found, or `None` if not found.
    ///
    pub fn table_find_by_id(&self, table_id: TableId) -> Option<Arc<Table>> {
        let inner = self.inner.lock().unwrap();
        inner.tables_by_id.get(&table_id).cloned()
    }

    /// Retrieves a table by its name from the cache.
    ///
    /// # Arguments
    ///
    /// * `table_name` - The name of the table to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<Arc<Table>>` containing the table if found, or `None` if not found.
    ///
    pub fn table_find_by_name(
        &self,
        table_name: &String,
    ) -> Option<Arc<Table>> {
        let inner = self.inner.lock().unwrap();
        inner.tables.get(table_name).cloned()
    }

    /// Retrieves an index by its name from the cache.
    ///
    /// # Arguments
    ///
    /// * `index_name` - The name of the index to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<Arc<Index>>` containing the table that has the index or `None` if not found.
    ///
    pub fn index_find_by_id(&self, index_id: IndexId) -> Option<Arc<Index>> {
        let inner = self.inner.lock().unwrap();
        for table in inner.tables_by_id.values() {
            for index in table.get_indexes() {
                if index.get_id() == index_id {
                    return Some(index.clone());
                }
            }
        }

        None
    }
}
