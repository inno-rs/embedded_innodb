use crate::buf::buf0types::BufBlock;
use crate::data::data0types::{
    DFieldBuilder, DTuple, DTupleBuilder, DTypeBuilder,
};
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::{UNIV_PAGE_SIZE, UNIV_SQL_NULL};
use crate::page::page0page::Page;

/// A structure that owns both the data vectors and the DTuple that references them.
/// 
/// This ensures that the data referenced by the DTuple remains valid for the lifetime
/// of the OwningDTuple instance.
pub struct OwningDTuple {
    /// Vectors containing the raw data for each field
    pub field_vectors: Vec<Vec<u8>>,
    /// The DTuple structure that references the data in field_vectors
    pub dtuple: DTuple,
}

/// A structure that owns both the page memory and the Page object that references it.
/// 
/// This ensures that the memory referenced by the Page remains valid for the lifetime
/// of the OwningPage instance.
pub struct OwningPage {
    /// Vector containing the raw page memory
    pub page_mem: Vec<u8>,
    /// The Page structure that references the memory in page_mem
    pub page: Page,
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
    let mut raw_memory = vec![0; UNIV_PAGE_SIZE];
    let index = Index::new();
    let buf_block = BufBlock::new(raw_memory.as_mut_ptr());

    OwningPage {
        page_mem: raw_memory,
        page: Page::page_create(&index, &buf_block, None),
    }
}

/// Creates a specified number of data tuples for testing purposes.
///
/// This function generates `n` data tuples, each with three fields:
/// - Two regular fields with 10 bytes of zeroed data
/// - One SQL NULL field
///
/// Each tuple is wrapped in an `OwningDTuple` to ensure the data remains valid
/// for the lifetime of the tuple.
///
/// # Parameters
///
/// * `n` - The number of data tuples to create
///
/// # Returns
///
/// A vector of `OwningDTuple` instances, each containing a data tuple with three fields.
pub fn create_dtuples(n: usize) -> Vec<OwningDTuple> {
    let mut result: Vec<OwningDTuple> = vec![];
    for _ in 0..n {
        let mut field1_data = vec![0u8, 10];
        let mut field2_data = vec![0u8; 10];
        let field3_data = vec![0u8; 10];
        let dtype = DTypeBuilder::default()
            .m_type(1)
            .len(10)
            .pr_type(1)
            .mb_min_len(0)
            .mb_max_len(1)
            .build()
            .unwrap();

        let dtuple_result = DTupleBuilder::default()
            .info_bits(1)
            .add_field(
                DFieldBuilder::default()
                    .data(field1_data.as_mut_ptr())
                    .len(field1_data.len())
                    .field_type(dtype.clone())
                    .build()
                    .unwrap(),
            )
            .add_field(
                DFieldBuilder::default()
                    .data(field2_data.as_mut_ptr())
                    .len(field2_data.len())
                    .field_type(dtype.clone())
                    .build()
                    .unwrap(),
            )
            .add_field(
                DFieldBuilder::default()
                    .data(field2_data.as_mut_ptr())
                    .len(UNIV_SQL_NULL)
                    .field_type(dtype.clone())
                    .build()
                    .unwrap(),
            )
            .n_fields(3)
            .build();

        let owning_tuple = OwningDTuple {
            field_vectors: vec![field1_data, field2_data, field3_data],
            dtuple: dtuple_result.unwrap(),
        };

        result.push(owning_tuple);
    }

    result
}
