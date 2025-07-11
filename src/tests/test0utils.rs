use std::{
    alloc::{Layout, alloc_zeroed},
    mem::discriminant,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    btr::{btr0btr::BTree, btr0cur::BTreeCursor},
    buf::{
        buf0buf::BufPool,
        buf0types::{BUF_GET_ALWAYS, BufBlock, BufPage, Request},
    },
    data::data0types::{
        DATA_NOT_NULL, DField, DFieldBuilder, DTYPE_DECIMAL, DTYPE_DOUBLE,
        DTYPE_FLOAT, DTYPE_INT, DTYPE_INT_UNSIGNED, DTYPE_VARCHAR, DTuple,
        DTupleBuilder, DType,
    },
    dict::dict0types::{Column, Field, Index},
    innodb::innodb0types::{PageId, PageNo, UNIV_PAGE_SIZE, UNIV_SQL_NULL},
    lock::lock0lock::LockSys,
    mach::mach0data::mach_write_to_4,
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{FixMode, MemoSlot},
    },
    page::{
        page0cur::{PAGE_CUR_LE, PageCur},
        page0page::{Page, PageDebug},
    },
    sync::sync0rw::ReadWriteLatch,
    ut::{
        ut0dbg::{ut_a, ut_ad},
        ut0rnd::ut_next_seed,
        ut0ut::random_string_with_seed,
    },
};

/// A structure that owns both the page memory and the Page object that references it.
///
/// This ensures that the memory referenced by the Page remains valid for the lifetime
/// of the OwningPage instance.
pub struct OwningPage {
    /// Vector containing the raw page memory
    pub page_mem: *mut u8,
    /// The Page structure that references the memory in page_mem
    pub page: Page,
    /// BufBlock containing page, and bufpage.
    pub buf_block: Arc<RwLock<BufBlock>>,
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
#[derive(Debug, Clone)]
pub enum FieldType {
    /// Signed 32-bit integer
    Int(Option<i32>),
    IntNullable(Option<i32>),
    /// Unsigned 64-bit integer
    UnsignedInt(Option<u64>),
    /// Binary data
    Binary(Option<Vec<u8>>),
    /// Variable-length string
    String(Option<String>),
    StringNullable,
    /// Decimal number
    Decimal(Option<String>),
    /// Single-precision floating point number
    Float(Option<f32>),
    FloatNullable(Option<f32>),
    /// Double-precision floating point number
    Double(Option<f64>),
    DoubleNullable(Option<f64>),
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
    let owning_page = create_raw_page();
    let buf_block = owning_page.buf_block.clone();

    create_records_in_page(
        n,
        fields,
        &owning_page.page,
        Arc::new(Index::new(PageId::dummy())),
        buf_block,
    );

    owning_page
}

/// Creates and inserts records into a page.
///
/// This function creates a specified number of data tuples with the given field types
/// and inserts them into the provided page. It uses a page cursor to insert each tuple
/// at the appropriate position in the page.
///
/// # Arguments
///
/// * `n` - The number of records to create and insert
/// * `fields` - The field types for each record
/// * `page` - The page to insert records into
/// * `index` - The index associated with the page
/// * `buf_block` - The buffer block containing the page
pub fn create_records_in_page(
    n: usize,
    fields: Vec<FieldType>,
    page: &Page,
    index: Arc<Index>,
    buf_block: Arc<RwLock<BufBlock>>,
) {
    let dtuples = create_dtuples(n, fields);

    let mut page_cur = PageCur::new(
        index,
        Some(page.page_get_infimum_rec()),
        buf_block,
        page.clone(),
    );

    let mut mtr = Mtr::new();
    let index = Index::new(PageId::dummy());
    for dtuple in dtuples {
        page_cur.page_cur_tuple_insert(&dtuple, &index, 0, &mut mtr);
        page_cur.page_cur_move_to_next();
    }
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
    let buf_block =
        Arc::new(RwLock::new(BufBlock::new(BufPage::dummy(), mem)));

    let buf_block_clone = buf_block.clone();
    OwningPage {
        page_mem: mem,
        page: Page::page_create(
            &index,
            buf_block.read().unwrap().deref(),
            None,
        ),
        buf_block: buf_block_clone,
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
pub fn create_dtuples(n: usize, fields: Vec<FieldType>) -> Vec<DTuple> {
    (0..n)
        .map(|_| -> DTuple {
            let dfields: Vec<DField> =
                fields.iter().map(|f| create_dfield(f)).collect();

            DTupleBuilder::default()
                .info_bits(0)
                .n_fields(dfields.len())
                .n_fields_cmp(dfields.len())
                .fields(dfields)
                .build()
                .unwrap()
        })
        .collect()
}

/// Creates data tuples from a chunk of input data.
///
/// This function takes a vector of arrays of field types and converts each array
/// into a data tuple. It first validates that all fields in each row have the same type,
/// then creates a data tuple for each row by mapping each field to a DField.
///
/// # Arguments
///
/// * `input_data` - A vector of arrays of field types, where each array represents a row
///
/// # Returns
///
/// A vector of data tuples created from the input data
///
/// # Panics
///
/// Panics if a row contains fields of different types
pub fn create_dtuples_from_input_data_chunk<const N: usize>(
    input_data: Vec<[FieldType; N]>,
) -> Vec<DTuple> {
    for (i, row) in input_data.iter().enumerate() {
        let expected = discriminant(&row[0]);

        for (j, field) in row.iter().enumerate() {
            ut_a!(
                discriminant(field) == expected,
                "Row {i} has inconsistent field type at column {j}"
            );
        }
    }

    input_data.iter().map(|input| create_dtuple(input)).collect()
}

/// Creates a data tuple from an array of field types for testing purposes.
///
/// This function builds a data tuple (DTuple) by converting each field type in the input array
/// to a data field (DField) using the `create_dfield` function. The resulting DTuple contains
/// all the fields with appropriate test data.
///
/// # Arguments
/// * `fields` - An array of field types that will be converted to data fields
///
/// # Returns
/// * A new DTuple containing the specified fields
pub fn create_dtuple<const N: usize>(fields: &[FieldType; N]) -> DTuple {
    let dfields: Vec<DField> =
        fields.iter().map(|f| create_dfield(f)).collect();

    DTupleBuilder::default()
        .info_bits(0)
        .n_fields(dfields.len())
        .n_fields_cmp(dfields.len())
        .fields(dfields)
        .build()
        .unwrap()
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
pub fn create_dfield(field_type: &FieldType) -> DField {
    match field_type {
        FieldType::Int(i) => {
            // INT (signed 32-bit), value = 0x12345678 = 305419896

            let data = i
                .or(Some(305419896))
                .map(|a| {
                    let mut buf = vec![0u8; a.to_le_bytes().len()];
                    mach_write_to_4(&mut buf, a as u32);
                    buf
                })
                .unwrap();

            let len = data.len();
            let mut typ = DTYPE_INT.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::IntNullable(ni) => {
            let data = ni
                .map(|a| {
                    let mut buf = vec![0u8; a.to_le_bytes().len()];
                    mach_write_to_4(&mut buf, a as u32);
                    buf
                })
                .unwrap_or(vec![]);

            let len = if data.len() == 0 { UNIV_SQL_NULL } else { data.len() };
            let mut typ = DTYPE_INT.clone();
            if len != UNIV_SQL_NULL {
                typ.pr_type |= DATA_NOT_NULL;
            }

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::UnsignedInt(ui) => {
            let data = ui
                .clone()
                .map(|a| a.to_le_bytes().to_vec())
                .unwrap_or(vec![1, 0, 0, 0, 0, 0, 0, 0]);
            let len = data.len();
            let mut typ = DTYPE_INT_UNSIGNED.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::Binary(bin) => {
            // BINARY(5), value = 01 02 03 04 05
            let data =
                bin.clone().unwrap_or(vec![0x01, 0x02, 0x03, 0x04, 0x05]);
            let len = data.len();
            let mut typ = DTYPE_VARCHAR.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::StringNullable => {
            let len = UNIV_SQL_NULL;
            let typ = DTYPE_VARCHAR.clone();

            DFieldBuilder::default()
                .data(vec![])
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::String(s) => {
            let data = s
                .clone()
                .or(Some("hello varchar".to_string()))
                .unwrap()
                .as_bytes()
                .to_vec();
            let len = data.len();
            let mut typ = DTYPE_VARCHAR.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::Decimal(d) => {
            let data = d
                .clone()
                .map(|a| a.as_bytes().to_vec())
                .unwrap_or(b"1234.5678".to_vec());

            let len = data.len();
            let mut typ = DTYPE_DECIMAL.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::Float(f) => {
            // FLOAT, value = 3.14f32 → 0x40490FDB (little-endian)
            let data = f
                .map(|a| a.to_le_bytes().to_vec())
                .unwrap_or(vec![0x40, 0x49, 0x0F, 0xDB]);

            let len = data.len();
            let mut typ = DTYPE_FLOAT.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::FloatNullable(f) => {
            let data = f.map(|a| a.to_le_bytes().to_vec()).unwrap_or(vec![]);

            let len = if data.len() == 0 { UNIV_SQL_NULL } else { data.len() };
            let mut typ = DTYPE_FLOAT.clone();
            if len != UNIV_SQL_NULL {
                typ.pr_type |= DATA_NOT_NULL;
            }

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::Double(d) => {
            // DOUBLE, value = 3.14f64 → 0x40091EB851EB851F
            let data = d.map(|a| a.to_le_bytes().to_vec()).unwrap_or(vec![
                0x40, 0x09, 0x1E, 0xB8, 0x51, 0xEB, 0x85, 0x1F,
            ]);

            let len = data.len();
            let mut typ = DTYPE_DOUBLE.clone();
            typ.pr_type |= DATA_NOT_NULL;

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
        }
        FieldType::DoubleNullable(d) => {
            let data = d.map(|a| a.to_le_bytes().to_vec()).unwrap_or(vec![]);

            let len = if data.len() == 0 { UNIV_SQL_NULL } else { data.len() };
            let mut typ = DTYPE_DOUBLE.clone();
            if data.len() != 0 {
                typ.pr_type |= DATA_NOT_NULL;
            }

            DFieldBuilder::default()
                .data(data)
                .len(len)
                .dtype(typ)
                .build()
                .unwrap()
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
    for i in 0..n {
        fields.push(Field {
            col: Arc::new(Mutex::new(Column {
                name: "".to_string(),
                index: 0,
                ord_part: 0,
                dtype: DType {
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
    }

    fields
}

/// Function to add some pages to the buffer pool.
///
/// This function initializes the buffer pool with a specified number of pages,
/// creates pages with unique space and page IDs, and verifies that the pages
/// can be retrieved from the buffer pool.
///
/// # Arguments
///
/// * `buf_pool` - The buffer pool to add blocks to
/// * `count` - The number of blocks to add to the buffer pool
pub fn create_blocks_in_buf_pool(buf_pool: Arc<Mutex<BufPool>>, count: usize) {
    let mut buf_pool = buf_pool.lock().unwrap();

    let mut mtr = Mtr::new();
    mtr.start();

    // Add some pages, and then retry creating.
    buf_pool.open(count * UNIV_PAGE_SIZE);
    ut_ad!(buf_pool.get_free_list_size() == count);
    ut_ad!(buf_pool.get_curr_size() == count * UNIV_PAGE_SIZE);

    for i in 0..count as PageNo {
        let resp = buf_pool.create(0, i, &mut mtr);
        ut_ad!(resp.is_some());
    }

    let mut prev_frame_address: Option<usize> = None;
    for i in 0..count as PageNo {
        let buf_block = buf_pool.get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id: 0, page_no: i },
            mode: BUF_GET_ALWAYS,
            mtr: &mut mtr,
        });

        if let Some(frame_address) = prev_frame_address {
            let current_address =
                buf_block.as_ref().unwrap().read().unwrap().get_frame()
                    as usize;
            ut_ad!(current_address == frame_address + UNIV_PAGE_SIZE);
        }

        prev_frame_address
            .replace(buf_block.as_ref().unwrap().read().unwrap().get_frame()
                as usize);

        ut_ad!(
            buf_block.is_some(),
            "Failed to get page for space_id: {}, page_no: {}",
            i,
            i
        );
    }
}

/// Fills a buffer block with uniform data for testing purposes.
///
/// This function populates a buffer block with a large number of records containing
/// random string data. It creates a B-tree cursor, positions it at the infimum record,
/// generates random string data, creates data tuples from this data, and inserts them
/// into the B-tree using optimistic insert.
///
/// # Arguments
/// * `btree` - The B-tree to insert records into
/// * `buf_block` - The buffer block to fill with data
/// * `buf_pool` - The buffer pool for managing pages
/// * `lock_sys` - The lock system for managing locks
/// * `seed` - A seed value for random string generation
pub fn fill_block_with_uniform_data(
    btree: Arc<BTree>,
    buf_block: Arc<RwLock<BufBlock>>,
    buf_pool: Arc<Mutex<BufPool>>,
    lock_sys: Arc<Mutex<LockSys>>,
    seed: u64,
) {
    let page: Page = buf_block.read().unwrap().get_page();
    let record = page.page_get_infimum_rec();

    ut_ad!(page.page_validate());

    let btree = btree.deref();
    let mut btr_cur =
        BTreeCursor::new(btree, btree.get_index().clone(), buf_pool, lock_sys);
    btr_cur.position(record, buf_block.clone());

    let low_limit = 0;
    let up_limit = 725;
    let mut input: Vec<[FieldType; 1]> = vec![];
    let mut current_seed = seed;
    for _ in low_limit..up_limit {
        let str = random_string_with_seed(15, current_seed);
        input.push([FieldType::String(Some(str))]);

        current_seed = ut_next_seed(current_seed);
    }

    let mut dtuples = create_dtuples_from_input_data_chunk(input);

    let mut mtr = Mtr::new();
    mtr.start();
    mtr.memo_push(MemoSlot::PageFix(
        PageId { space_id: 0, page_no: 0 },
        FixMode::Exclusive,
    ));

    for dtuple in dtuples.iter_mut() {
        btr_cur.search_to_nth_level(0, dtuple, PAGE_CUR_LE, 0, &mut mtr);

        btr_cur.optimistic_insert(0, dtuple, 0, &mut mtr);
    }

    let user_recs = page.page_get_user_records();

    // ut_ad!(all_equal_by(&user_recs, |a: &Record, b: &Record| {
    //     a.rec_offs_size() == b.rec_offs_size()
    // }));
}
