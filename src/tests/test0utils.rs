use std::alloc::{Layout, alloc_zeroed};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::buf::buf0types::{BufBlock, BufPage};
use crate::data::data0types::{
    DATA_NOT_NULL, DField, DFieldBuilder, DTYPE_DECIMAL, DTYPE_DOUBLE,
    DTYPE_FLOAT, DTYPE_INT, DTYPE_INT_UNSIGNED, DTYPE_VARCHAR, DTuple, DType,
};
use crate::dict::dict0types::{Column, Field, Index};
use crate::innodb::innodb0types::{PageId, UNIV_PAGE_SIZE, UNIV_SQL_NULL};
use crate::mtr::mtr0mtr::Mtr;
use crate::page::page0cur::PageCur;
use crate::page::page0page::Page;
use crate::rem::rem0rec::Record;
use crate::some_or_panic;

/// A structure that owns both the data vectors and the DTuple that references them.
///
/// This ensures that the data referenced by the DTuple remains valid for the lifetime
/// of the OwningDTuple instance.
pub struct OwningDTuple {
    /// The DTuple structure that references the data in field_vectors
    pub dtuple: DTuple,
}

/// A structure that owns both the data vector and the DField that references it.
///
/// This ensures that the data referenced by the DField remains valid for the lifetime
/// of the OwningDField instance.
pub struct OwningDField {
    /// Vector containing the raw field data
    pub field_data_owner: Vec<u8>,
    /// The DField structure that references the data in field_data_owner
    pub field: DField,
}

/// A structure that owns both the page memory and the Page object that references it.
///
/// This ensures that the memory referenced by the Page remains valid for the lifetime
/// of the OwningPage instance.
pub struct OwningPage {
    /// Vector containing the raw page memory
    pub page_mem: *mut u8,
    /// The Page structure that references the memory in page_mem
    pub page: Page,
    pub buf_block: BufBlock,
}

impl Drop for OwningPage {
    fn drop(&mut self) {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { std::alloc::dealloc(self.page_mem, layout) }
    }
}

/// Enum representing different field types that can be used in test data.
///
/// This enum is used to specify the type of field to create when generating
/// test data structures.
pub enum FieldType {
    /// Signed 32-bit integer
    Int,
    /// Unsigned 64-bit integer
    UnsignedInt,
    /// Binary data
    Binary,
    /// Variable-length string
    String,
    StringNull,
    /// Decimal number
    Decimal,
    /// Single-precision floating point number
    Float,
    /// Double-precision floating point number
    Double,
}

/// Create records.
///
/// # Arguments
///
/// `n`: Number of records.
/// `fields`: Type of field in each record.
///
/// # Returns
///
/// `OwningPage`: A page that contains the newly created records.
pub fn create_records(n: usize, fields: Vec<FieldType>) -> OwningPage {
    let dtuples = create_dtuples(n, fields);

    let owning_page = create_raw_page();
    let buf_block = owning_page.buf_block.clone();

    let mut page_cur = PageCur::new(
        Arc::new(Index::new(PageId::dummy())),
        owning_page.page.page_get_infimum_rec(),
        buf_block,
        owning_page.page,
    );

    let mut mtr = Mtr::new();
    let index = Index::new(PageId::dummy());
    for owning_dtuple in dtuples {
        page_cur.page_cur_tuple_insert(
            &owning_dtuple.dtuple,
            &index,
            0,
            &mut mtr,
        );
        page_cur.page_cur_move_to_next();
    }

    owning_page
}

/// Creates a new empty page for testing purposes.
///
/// This function allocates memory for a page of size `UNIV_PAGE_SIZE`, initializes it,
/// and returns an `OwningPage` that contains both the raw memory and the initialized Page.
///
/// # Returns
///
/// An `OwningPage` instance containing the newly created page and its memory.
pub fn create_raw_page() -> OwningPage {
    let layout =
        Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
    let mem = unsafe { alloc_zeroed(layout) };

    let index = Index::new(PageId::dummy());
    let buf_block = BufBlock::new(
        RefCell::new(Rc::new(RefCell::new(BufPage::dummy()))),
        mem,
    );

    OwningPage {
        page_mem: mem,
        page: Page::page_create(&index, &buf_block, None),
        buf_block,
    }
}

/// Creates a specified number of data tuples for testing purposes.
///
/// This function generates `n` data tuples, each with fields of the types specified
/// in the `fields` parameter.
/// Each tuple is wrapped in an `OwningDTuple` to ensure
/// the data remains valid for the lifetime of the tuple.
///
/// # Arguments
///
/// * `n` - The number of data tuples to create
/// * `fields` - A vector of field types to include in each tuple
///
/// # Returns
///
/// A vector of `OwningDTuple` instances, each containing a data tuple with
/// fields of the specified types.
pub fn create_dtuples(n: usize, fields: Vec<FieldType>) -> Vec<OwningDTuple> {
    (0..n)
        .map(|_| -> OwningDTuple {
            let owning_fields: Vec<OwningDField> =
                fields.iter().map(|f| create_dfield(f)).collect();
            let dfields: Vec<DField> =
                owning_fields.iter().map(|of| of.field.clone()).collect();

            OwningDTuple {
                dtuple: DTuple {
                    info_bits: 0,
                    n_fields: dfields.len(),
                    fields: dfields,
                },
            }
        })
        .collect()
}

/// Creates a data field of the specified type for testing purposes.
///
/// This function generates a data field with appropriate test data based on the
/// specified field type. The field is wrapped in an `OwningDField` to ensure
/// the data remains valid for the lifetime of the field.
///
/// # Arguments
///
/// * `field_type` - The type of field to create
///
/// # Returns
///
/// An `OwningDField` instance containing the newly created field with test data.
pub fn create_dfield(field_type: &FieldType) -> OwningDField {
    match field_type {
        FieldType::Int => {
            // INT (signed 32-bit), value = 0x12345678 = 305419896

            let data = vec![0x12, 0x34, 0x56, 0x78];
            let len = data.len();
            let mut typ = DTYPE_INT.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::UnsignedInt => {
            let data = vec![1, 0, 0, 0, 0, 0, 0, 0];
            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_INT_UNSIGNED.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::Binary => {
            // BINARY(5), value = 01 02 03 04 05

            let data = vec![0x01, 0x02, 0x03, 0x04, 0x05];
            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_VARCHAR.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::StringNull => {
            let len = UNIV_SQL_NULL;
            let typ = DTYPE_VARCHAR.clone();

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(vec![])
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::String => {
            let data = b"hello varchar".to_vec();
            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_VARCHAR.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::Decimal => {
            let data = b"1234.5678".to_vec();

            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_DECIMAL.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::Float => {
            // FLOAT, value = 3.14f32 → 0x40490FDB (little-endian)
            let data = vec![0x40, 0x49, 0x0F, 0xDB];

            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_FLOAT.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
        FieldType::Double => {
            // DOUBLE, value = 3.14f64 → 0x40091EB851EB851F
            let data = vec![0x40, 0x09, 0x1E, 0xB8, 0x51, 0xEB, 0x85, 0x1F];

            let data_ptr = data.as_ptr();
            let len = data.len();
            let mut typ = DTYPE_DOUBLE.clone();
            typ.pr_type |= DATA_NOT_NULL;

            OwningDField {
                field_data_owner: vec![],
                field: DFieldBuilder::default()
                    .data(data)
                    .len(len)
                    .dtype(typ)
                    .build()
                    .unwrap(),
            }
        }
    }
}

/// Create new fields for index.
///
/// # Arguments
///
/// `n`: The total number of fields.
///
/// # Returns
///
/// `Vec<Field>` containing all the newly created fields.
pub fn create_new_fields(n: usize) -> Vec<Field> {
    let mut fields = Vec::<Field>::new();
    fields.push(Field {
        col: Arc::new(Mutex::new(Column {
            name: "".to_string(),
            index: 0,
            ord_part: 0,
            d_type: DType {
                m_type: 0,
                len: 0,
                pr_type: 0,
                mb_min_len: 0,
                mb_max_len: 0,
            },
        })),
        name: "".to_string(),
        prefix_length: 0,
        fixed_length: 0,
    });

    fields
}

/// Get all the user records on the page.
///
/// # Arguments
///
/// `page`: Page containing the records.
///
/// # Returns
///
/// `Vec<Record>` A vector of user records (excluding infimum and supremum).
pub fn get_user_records(page: Page) -> Vec<Record> {
    let mut result = Vec::<Record>::new();
    let mut cur_rec = Some(page.page_get_infimum_rec());
    while cur_rec.is_some() {
        let rec = some_or_panic!(cur_rec);
        if page.page_rec_is_user_rec(rec) {
            result.push(rec);
        }

        cur_rec = cur_rec.unwrap().rec_get_next();
    }

    result
}
