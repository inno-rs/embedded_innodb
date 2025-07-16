use std::{
    fmt::Display,
    sync::{Arc, Mutex},
};

use crate::{
    data::data0types::{
        DATA_NOT_NULL, DATA_SYS_CHILD, DFieldBuilder, DTuple, DType,
    },
    innodb::innodb0types::{PageId, PageNo},
    rem::rem0rec::{REC_STATUS_NODE_PTR, Record},
    some_or_panic,
    ut::ut0dbg::ut_ad,
};

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
#[derive(Copy, Clone)]
pub enum IndexType {
    /// Clustered Index.
    DictClustered,
    /// Unique Index.
    DictUnique,

    DictUniversal,
}

pub struct Index {
    /// Index id.
    id: usize,
    /// The page ID of the root page of this index in the B-tree structure.
    page_id: PageId,
    /// Inner struct which requires mutability.
    inner: Arc<Mutex<IndexInner>>,
}

/// Internal structure of an index that requires mutability.
///
/// This struct contains the mutable components of an index, including its type,
/// field counts, and the actual fields. It is wrapped in an Arc<Mutex<>> in the
/// Index struct to allow for thread-safe mutability.
pub struct IndexInner {
    /// Type of index,
    index_type: IndexType,
    /// Number of user defined columns in the index. In the internal representation, we have more.
    n_user_defined_cols: usize,
    /// Number of fields from the beginning that can help determine an entry in the index uniquely.
    n_uniq: usize,
    /// Number of fields defined in the index.
    n_defined: usize,
    /// Number of nullable fields.
    n_nullable: usize,
    /// Fields that make up the index.
    fields: Vec<Field>,
}

impl Index {
    pub fn new(page_id: PageId) -> Self {
        Self {
            id: 0,
            page_id,
            inner: Arc::new(Mutex::new(IndexInner {
                index_type: IndexType::DictClustered,
                n_user_defined_cols: 0,
                n_uniq: 0,
                n_defined: 0,
                n_nullable: 0,
                fields: vec![],
            })),
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
        let inner = self.inner.lock().unwrap();
        if n < inner.fields.len() {
            inner.fields[n].col.clone()
        } else {
            panic!("Column index out of range");
        }
    }

    /// Returns the unique identifier of this index.
    ///
    /// # Returns
    /// The index ID as a usize
    pub fn get_index_id(&self) -> usize {
        self.id
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

    /// Returns the page identifier for this index.
    ///
    /// The page ID consists of a space ID and a page number, uniquely identifying
    /// the page where this index is stored.
    ///
    /// # Returns
    /// The page ID of this index
    pub fn get_page_id(&self) -> PageId {
        self.page_id
    }

    /// Returns the type of this index.
    ///
    /// The index type can be clustered, unique, or universal, which determines
    /// how the index is structured and used.
    ///
    /// # Returns
    /// The type of this index as an IndexType enum
    pub fn index_type(&self) -> IndexType {
        self.inner.lock().unwrap().index_type.clone()
    }

    /// Returns the number of user-defined columns in this index.
    ///
    /// User-defined columns are those explicitly specified by the user when
    /// creating the index, as opposed to system-generated columns.
    ///
    /// # Returns
    /// The number of user-defined columns
    pub fn n_user_defined_cols(&self) -> usize {
        self.inner.lock().unwrap().n_user_defined_cols
    }

    /// Returns the number of columns that form the unique key for this index.
    ///
    /// This value represents how many columns are needed to uniquely identify
    /// a record in this index.
    ///
    /// # Returns
    /// The number of columns forming the unique key
    pub fn n_uniq(&self) -> usize {
        self.inner.lock().unwrap().n_uniq
    }

    /// Sets the number of columns that form the unique key for this index.
    ///
    /// This method updates the number of columns needed to uniquely identify
    /// a record in this index.
    ///
    /// # Parameters
    /// * `n_uniq` - The new number of columns forming the unique key
    pub fn set_n_uniq(&self, n_uniq: usize) {
        self.inner.lock().unwrap().n_uniq = n_uniq;
    }

    /// Returns the number of defined columns in this index.
    ///
    /// Defined columns are those that have been explicitly defined with a data type
    /// and other attributes in the index schema.
    ///
    /// # Returns
    /// The number of defined columns in the index
    pub fn n_defined(&self) -> usize {
        self.inner.lock().unwrap().n_defined
    }

    /// Returns the number of nullable columns in this index.
    ///
    /// Nullable columns are those that can contain NULL values. This count is important
    /// for index operations and storage optimizations.
    ///
    /// # Returns
    /// The number of nullable columns in the index
    pub fn n_nullable(&self) -> usize {
        self.inner.lock().unwrap().n_nullable
    }

    /// Returns the page identifier for this index.
    ///
    /// This is an alias for get_page_id() that provides the same functionality.
    /// The page ID uniquely identifies the page where this index is stored.
    ///
    /// # Returns
    /// The page ID of this index
    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    /// Adds a new field to this index.
    ///
    /// This method appends a field to the list of fields in the index.
    /// Fields represent the columns that make up the index.
    ///
    /// # Parameters
    /// * `field` - The field to add to the index
    pub fn add_field(&self, field: Field) {
        self.inner.lock().unwrap().fields.push(field);
    }

    /// Determines if this index is a clustered index.
    ///
    /// A clustered index is one where the data rows are stored in the same order
    /// as the index. This is in contrast to secondary indexes, which point to the
    /// primary key.
    ///
    /// # Returns
    /// `true` if this is a clustered index, `false` otherwise
    pub fn is_clustered(&self) -> bool {
        match self.inner.lock().unwrap().index_type {
            IndexType::DictClustered => true,
            IndexType::DictUnique | IndexType::DictUniversal => false,
        }
    }

    /// Get the number of columns that uniquely identify the record.
    ///
    /// # Returns
    /// Number of fields that uniquely identify the record.
    /// `n_uniq` if clustered.
    /// Otherwise, all fields.
    pub fn get_n_unique_in_tree(&self) -> usize {
        let is_clustered = self.is_clustered();
        let inner = self.inner.lock().unwrap();
        match is_clustered {
            true => inner.n_uniq,
            false => inner.fields.len(),
        }
    }

    /// Copy the types of the fields in the index to dtuple.
    ///
    /// # Arguments
    ///
    /// `dtuple`: A DTuple to which we copy the types.
    /// `n_fields`: Number of fields to copy.
    ///
    pub fn copy_types(&self, dtuple: &mut DTuple, n_fields: usize) {
        let inner = self.inner.lock().unwrap();
        for i in 0..n_fields {
            let field = some_or_panic!(inner.fields.get(i));
            let dest_field = some_or_panic!(dtuple.fields.get_mut(i));
            let col = field.col.lock().unwrap();
            dest_field.dtype = col.dtype.clone();
        }
    }

    /// Build node ptr that represents a non-leaf node.
    /// Non-leaf nodes contain the required field for comparison,
    /// and the last fields is the child node page node.
    ///
    /// # Arguments
    ///
    /// `rec`: The record from which we need to construct the dtuple.
    /// `page_no`: Child page no to put in the record.
    ///
    /// # Returns
    ///
    /// `DTuple` containing the contents of the record and the child page number in addition.
    pub fn build_inner_node(&self, rec: Record, page_no: PageNo) -> DTuple {
        let n_unique = self.get_n_unique_in_tree();
        // We are creating a non-leaf node.
        // The last field is the page no to the child nodes of the btree.
        let mut dtuple = DTuple::new(n_unique + 1);

        // We are creating a non-leaf node.
        // And as we mentioned above, we created an extra field to store the child page node.
        // When searching in the index tree for the node pointer,
        // we should not compare the last field.
        dtuple.dtuple_set_n_fields_cmp(n_unique);

        self.copy_types(&mut dtuple, n_unique);

        let nth_field = dtuple.dtuple_get_nth_field_mut(n_unique);
        nth_field.dfield_set_data(page_no.to_le_bytes().to_vec(), 4);

        let dtype = &mut nth_field.dtype;
        dtype.m_type = DATA_SYS_CHILD as u16;
        dtype.pr_type = DATA_NOT_NULL;
        dtype.len = 4;

        rec.rec_copy_prefix_to_dtuple(n_unique, &mut dtuple);

        dtuple.dtuple_set_info_bits(
            dtuple.dtuple_get_info_bits() | REC_STATUS_NODE_PTR,
        );

        ut_ad!(dtuple.dtuple_type_checked());

        dtuple
    }

    /// Builds the smallest possible inner node for a B-tree index.
    ///
    /// This method creates a data tuple that represents the smallest possible
    /// inner node in a B-tree, containing only a reference to a child page.
    /// It's typically used when creating new index pages or during page splits.
    ///
    /// # Parameters
    /// * `page_no` - The page number of the child page to reference
    ///
    /// # Returns
    /// A data tuple representing the smallest inner node
    pub fn build_smallest_inner_node(&self, page_no: PageNo) -> DTuple {
        let dfield = DFieldBuilder::default()
            .data(page_no.to_le_bytes().to_vec())
            .len(4)
            .ext(false)
            .dtype(DType {
                m_type: DATA_SYS_CHILD as u16,
                pr_type: DATA_NOT_NULL,
                len: 4,
                mb_min_len: 0,
                mb_max_len: 0,
            })
            .build()
            .unwrap();

        let mut dtuple = DTuple::dtuple_get_smallest(vec![dfield]);
        dtuple.dtuple_set_info_bits(
            REC_STATUS_NODE_PTR | dtuple.dtuple_get_info_bits(),
        );

        dtuple
    }

    /// Builds a typed data tuple out of a physical record.
    /// Copy all the fields.
    ///
    /// # Arguments
    ///
    /// `rec`: The record from which we need to construct the dtuple.
    /// `page_no`: Child page no to put in the record.
    ///
    /// # Returns
    ///
    /// `DTuple` containing the contents of the record.
    pub fn build_data_tuple(&self, rec: Record) -> DTuple {
        let n_fields = rec.rec_get_n_fields();
        let mut dtuple = DTuple::new(n_fields);

        self.copy_types(&mut dtuple, n_fields);

        rec.rec_copy_prefix_to_dtuple(n_fields, &mut dtuple);

        ut_ad!(dtuple.dtuple_type_checked());

        dtuple
    }

    /// Verifies that a data tuple is valid for searching in this index.
    ///
    /// This function checks that the number of fields in the tuple that will be
    /// compared during a search does not exceed the number of unique fields in the index.
    ///
    /// # Arguments
    /// * `dtuple` - The data tuple to check
    ///
    /// # Returns
    /// `true` if the tuple is valid for searching, asserts in debug mode if invalid
    pub fn check_search_tuple(&self, dtuple: &DTuple) -> bool {
        ut_ad!(
            dtuple.dtuple_get_n_fields_cmp() <= self.get_n_unique_in_tree()
        );

        true
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
    pub dtype: DType,
}

pub struct Table {}

impl Display for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table")
    }
}
