use std::{
    fmt::Display,
    sync::{Arc, Mutex},
};

use crate::{
    const_assert,
    data::data0types::{
        DATA_N_SYS_COLS, DATA_NOT_NULL, DATA_ROLL_PTR, DATA_ROLL_PTR_LEN,
        DATA_ROW_ID, DATA_ROW_ID_LEN, DATA_SYS, DATA_TRX_ID, DATA_TRX_ID_LEN,
        DType,
    },
    dict::dict0types::Column,
    ut::ut0dbg::ut_ad,
};

/// A unique identifier for a table.
///
/// This is a wrapper around a u64 value that provides type safety.
#[derive(Hash, Clone, Copy, Eq, PartialEq)]
pub struct TableId(pub u64);

/// Statistics for table.
pub struct TableStats {
    /// Approximate number of rows in the table. This is calculated periodically.
    n_rows: u64,

    /// Approximate clustered index size in database pages.
    clustered_index_size: u64,

    /// Sum of secondary index sizes.
    sum_of_secondary_index_sizes: u64,

    /// This counter is incremented every time a row is inserted, updated, or deleted.
    modified_counter: u64,

    /// True if the statistics have been calculated for the first time after database startup or table creation.
    initialized: bool,
}

/// Table struct for maintaining metadata about the table.
pub struct Table {
    /// Table identifier.
    id: TableId,

    /// Reference to the inner mutable fields.
    inner: Arc<Mutex<TableInner>>,
}

struct TableInner {
    /// Table name
    name: String,
    /// Table statistics.
    stats: TableStats,
    /// Number of fields in the table.
    n_fields: usize,
    /// Number of columns.
    n_cols: usize,
    /// Columns of the table.
    cols: Vec<Arc<Column>>,
}

impl Table {
    /// Creates a new table with the given ID and name.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier for the table
    /// * `name` - The name of the table
    ///
    /// # Returns
    ///
    /// A new `Table` instance with initialized empty statistics and no columns.
    ///
    pub fn new(id: u64, name: String) -> Self {
        let mut table = Table {
            id: TableId(id),
            inner: Arc::new(Mutex::new(TableInner {
                name,
                stats: TableStats {
                    n_rows: 0,
                    clustered_index_size: 0,
                    sum_of_secondary_index_sizes: 0,
                    modified_counter: 0,
                    initialized: false,
                },
                n_fields: 0,
                n_cols: 0,
                cols: vec![],
            })),
        };
        table.add_system_columns();

        table
    }

    /// Returns the unique identifier of the table.
    ///
    /// # Returns
    ///
    /// The table's ID.
    ///
    pub fn get_id(&self) -> TableId {
        self.id
    }

    /// Returns the name of the table.
    ///
    /// # Returns
    ///
    /// A string containing the table's name.
    ///
    pub fn get_name(&self) -> String {
        self.inner.lock().unwrap().name.to_string()
    }

    /// Returns the number of system columns in the table.
    ///
    /// # Returns
    ///
    /// The count of system columns.
    ///
    pub fn get_n_sys_cols(&self) -> usize {
        DATA_N_SYS_COLS
    }

    /// Returns the column at the specified position.
    ///
    /// # Arguments
    ///
    /// * `pos` - The position (index) of the column to retrieve
    ///
    /// # Returns
    ///
    /// A reference-counted pointer to the column.
    ///
    pub fn get_nth_col(&self, pos: usize) -> Arc<Column> {
        self.inner.lock().unwrap().get_nth_col(pos)
    }

    /// Get the name of the column using position.
    ///
    /// # Arguments
    ///
    /// `pos`: Index of the column.
    ///
    /// # Returns
    ///
    /// Name of the column.
    ///
    pub fn get_col_name(&self, pos: usize) -> String {
        let inner = self.inner.lock().unwrap();
        ut_ad!(pos < inner.n_cols);
        inner.cols[pos].name.to_string()
    }

    /// Get the column name from the name.
    ///
    /// # Arguments
    ///
    /// `name`: Name of the column.
    ///
    /// # Returns
    ///
    /// The index of the column if it exists. None, otherwise.
    ///
    pub fn get_col_no(&self, name: &str) -> Option<usize> {
        let inner = self.inner.lock().unwrap();
        for (index, col) in inner.cols.iter().enumerate() {
            if col.name == name {
                return Some(index);
            }
        }

        None
    }

    /// Returns the number of user-defined columns in the table.
    ///
    /// This excludes system columns.
    ///
    /// # Returns
    ///
    /// The count of user-defined columns.
    ///
    pub fn get_n_user_cols(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.n_cols - DATA_N_SYS_COLS
    }

    /// Add a column to the definition of the table.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the column.
    /// * `m_type` - Main type of the column.
    /// * `pr_type` - Precision type of the column.
    /// * `len` - Length of the column.
    ///
    pub fn add_col(
        &self,
        name: &str,
        m_type: usize,
        pr_type: usize,
        len: usize,
    ) {
        ut_ad!(name.len() != 0);

        let mut inner = self.inner.lock().unwrap();
        let col_index = inner.n_cols;

        let col = Arc::new(Column {
            name: name.to_string(),
            index: col_index,
            ord_part: 0,
            dtype: DType {
                m_type: m_type as u16,
                len: len as u16,
                pr_type,
                mb_min_len: 0,
                mb_max_len: 0,
            },
        });

        inner.cols.push(col.clone());
        inner.incr_n_cols();
    }

    /// Adds the standard system columns to the table.
    ///
    /// This method adds three system columns to the table:
    /// - DB_ROW_ID: Row identifier
    /// - DB_TRX_ID: Transaction identifier
    /// - DB_ROLL_LTR: Rollback pointer
    ///
    /// These columns are required for InnoDB's MVCC (Multi-Version Concurrency Control).
    fn add_system_columns(&mut self) {
        const_assert!(DATA_ROW_ID == 0, "error DATA_ROW_ID != 0");
        const_assert!(DATA_TRX_ID == 1, "error DATA_TRX_ID != 0");
        const_assert!(DATA_ROLL_PTR == 2, "error DATA_ROLL_PTR != 0");
        const_assert!(DATA_N_SYS_COLS == 3, "error DATA_N_SYS_COLS != 0");

        self.add_col(
            "DB_ROW_ID",
            DATA_SYS,
            DATA_ROW_ID | DATA_NOT_NULL,
            DATA_ROW_ID_LEN,
        );

        self.add_col(
            "DB_TRX_ID",
            DATA_SYS,
            DATA_TRX_ID | DATA_NOT_NULL,
            DATA_TRX_ID_LEN,
        );

        self.add_col(
            "DB_ROLL_PTR",
            DATA_SYS,
            DATA_ROLL_PTR | DATA_NOT_NULL,
            DATA_ROLL_PTR_LEN,
        );
    }
}

impl TableInner {
    /// Increments the column count of the table.
    ///
    /// This method is called when a new column is added to the table.
    pub fn incr_n_cols(&mut self) {
        self.n_cols += 1;
    }

    /// Returns the column at the specified position.
    ///
    /// # Arguments
    ///
    /// * `pos` - The position (index) of the column to retrieve
    ///
    /// # Returns
    ///
    /// A reference-counted pointer to the column.
    ///
    pub fn get_nth_col(&self, pos: usize) -> Arc<Column> {
        self.cols[pos].clone()
    }
}

/// Implements the Display trait for Table to provide a string representation.
impl Display for Table {
    /// Formats the table for display.
    ///
    /// # Arguments
    ///
    /// * `f` - The formatter to write to
    ///
    /// # Returns
    ///
    /// A Result indicating whether the formatting was successful.
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table")?;
        write!(f, "id: {}", self.id.0)?;
        write!(f, "name: {}", self.get_name())
    }
}

#[cfg(test)]
mod tests {
    use crate::{data::data0types::DATA_N_SYS_COLS, dict::dict0table::Table};

    #[test]
    fn test_table() {
        let table = Table::new(1, "test".to_string());
        assert_eq!(table.get_id().0, 1);
        assert_eq!(table.get_name(), "test");
        assert_eq!(table.get_n_user_cols(), 0);
        assert_eq!(table.get_n_sys_cols(), DATA_N_SYS_COLS);
    }

    #[test]
    fn test_table_add_col() {
        let table = Table::new(1, "test".to_string());

        assert_eq!(table.get_n_user_cols(), 0);
        assert_eq!(table.get_n_sys_cols(), DATA_N_SYS_COLS);

        table.add_col("col_1", 1, 1, 1);
        assert_eq!(table.get_n_user_cols(), 1);
        assert_eq!(table.get_n_sys_cols(), DATA_N_SYS_COLS);

        table.add_col("col_2", 1, 1, 1);
        assert_eq!(table.get_n_user_cols(), 2);
        assert_eq!(table.get_n_sys_cols(), DATA_N_SYS_COLS);
    }

    #[test]
    fn test_find_col_by_pos() {
        let table = Table::new(1, "test".to_string());
        table.add_col("col_1", 1, 1, 1);
        table.add_col("col_2", 1, 1, 1);
        table.add_col("col_3", 1, 1, 1);

        // Check existing one.
        assert_eq!(table.get_col_name(0), "DB_ROW_ID");
        assert_eq!(table.get_col_name(1), "DB_TRX_ID");
        assert_eq!(table.get_col_name(2), "DB_ROLL_PTR");

        assert_eq!(table.get_col_name(3), "col_1");
        assert_eq!(table.get_col_name(4), "col_2");
        assert_eq!(table.get_col_name(5), "col_3");
    }

    #[test]
    fn test_find_col_by_name() {
        let table = Table::new(1, "test".to_string());
        table.add_col("col_1", 1, 1, 1);
        table.add_col("col_2", 1, 1, 1);
        table.add_col("col_3", 1, 1, 1);

        // Check existing ones.
        assert_eq!(table.get_col_no("DB_ROW_ID"), Some(0));
        assert_eq!(table.get_col_no("DB_TRX_ID"), Some(1));
        assert_eq!(table.get_col_no("DB_ROLL_PTR"), Some(2));
        assert_eq!(table.get_col_no("col_1"), Some(3));
        assert_eq!(table.get_col_no("col_2"), Some(4));
        assert_eq!(table.get_col_no("col_3"), Some(5));

        // Check non-existing ones.
        assert_eq!(table.get_col_no("test1"), None);
        assert_eq!(table.get_col_no("test2"), None);
    }
}
