use crate::data::data0types::DType;
use crate::innodb::innodb0types::{PageId, PageNo};
use std::sync::{Arc, Mutex};

pub struct Field {
    /// Pointer to the column.
    col: Arc<Mutex<Column>>,
    /// Name of the column.
    pub name: String,
    /// 0 or the length of the column prefix in bytes.
    /// E.g., for Index(textcol(25)); must be smaller than DIC_MAX_INDEX_COL_LEN.
    pub prefix_length: usize,
    /// 0 or the fixed length of the column if smaller than DICT_MAX_INDEX_COL_LEN.
    pub fixed_length: usize,
}
pub struct Index {
    fields: Vec<Field>,
    /// The page ID of the root page of this index in the B-tree structure.
    page_id: PageId,
}

impl Index {
    pub(crate) fn new(page_id: PageId) -> Self {
        Index { fields: vec![], page_id }
    }

    /// Gets the nth column of a table.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the column to retrieve.
    ///
    /// # Returns
    ///
    /// * The nth column of the table.
    ///     
    pub fn get_nth_col(&self, n: usize) -> Arc<Mutex<Column>> {
        if n < self.fields.len() {
            self.fields[n].col.clone()
        } else {
            panic!("Column index out of range");
        }
    }

    /// Retrieves the page number of the root page of this index.
    ///
    /// The root page is the top-level page in the B-tree structure of the index.
    /// This method is used when traversing the B-tree from the root down to the leaves.
    ///
    /// # Returns
    ///
    /// The page number of the root page of this index.
    pub fn get_root_page_no(&self) -> PageNo {
        self.page_id.page_no
    }

    pub fn is_clustered(&self) -> bool {
        false
    }
}

pub struct Column {
    /// Column name.
    pub name: String,
    /// Table column position.
    pub index: usize,
    /// Nonzero if this column appears in the ordering fields of the index.
    pub ord_part: usize,
    /// Type of the column.
    pub d_type: DType,
}

pub struct Table {}
