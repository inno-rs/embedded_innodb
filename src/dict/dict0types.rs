use std::sync::{Arc, Mutex};

use crate::data::data0types::DType;
use crate::innodb::innodb0types::{PageId, PageNo};

pub struct Field {
    /// Pointer to the column.
    pub col: Arc<Mutex<Column>>,
    /// Name of the column.
    pub name: String,
    /// 0 or the length of the column prefix in bytes.
    /// E.g., for Index(textcol(25)); must be smaller than DIC_MAX_INDEX_COL_LEN.
    pub prefix_length: usize,
    /// 0 or the fixed length of the column if smaller than DICT_MAX_INDEX_COL_LEN.
    pub fixed_length: usize,
}

/// Type of index.
pub enum IndexType {
    /// Clustered Index.
    DictClustered,
    /// Unique Index.
    DictUnique,

    DictUniversal,
}

pub struct Index {
    /// Type of index,
    index_type: IndexType,
    /// Number of user defined columns in the index. In the internal representation, we have more.
    n_user_defined_cols: usize,
    /// Number of fields from the beginning that can help determine an entry in the index uniquely.
    n_uniq: usize,
    n_defined: usize,
    /// Number of nullable fields.
    n_nullable: usize,
    /// Fields.
    fields: Vec<Field>,
    /// The page ID of the root page of this index in the B-tree structure.
    page_id: PageId,
}

impl Index {
    pub fn new(page_id: PageId) -> Self {
        Self {
            index_type: IndexType::DictClustered,
            n_user_defined_cols: 0,
            n_uniq: 0,
            n_defined: 0,
            n_nullable: 0,
            fields: vec![],
            page_id,
        }
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

    pub fn get_page_id(&self) -> PageId {
        self.page_id
    }

    pub fn index_type(&self) -> &IndexType {
        &self.index_type
    }

    pub fn n_user_defined_cols(&self) -> usize {
        self.n_user_defined_cols
    }

    pub fn n_uniq(&self) -> usize {
        self.n_uniq
    }

    pub fn n_defined(&self) -> usize {
        self.n_defined
    }

    pub fn n_nullable(&self) -> usize {
        self.n_nullable
    }

    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    pub fn add_field(&mut self, field: Field) {
        self.fields.push(field);
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
