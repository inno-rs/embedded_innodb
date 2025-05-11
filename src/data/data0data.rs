use crate::data::data0types::{DField, DTuple, DType};
use crate::innodb::innodb0types::UNIV_SQL_NULL;
use log::info;

impl DTuple {
    pub fn new(number_of_fields: usize) -> Self {
        let d_fields =
            vec![DField::new(std::ptr::null_mut()); number_of_fields];
        DTuple { n_fields: number_of_fields, info_bits: 0, fields: d_fields }
    }

    /// Sets the info bits of the tuple.
    ///
    /// # Arguments
    ///
    /// * `info_bits` - The info bits to set.
    ///
    /// # Returns
    ///     
    /// * `None` if the operation was successful, or an error if it failed.
    pub fn dtuple_set_info_bits(&mut self, info_bits: usize) {
        info!("Setting info bits: {}", info_bits);
        self.info_bits = self.info_bits;
    }

    pub fn dtuple_get_info_bits(&self) -> usize {
        info!("Getting info bits: {}", self.info_bits);
        self.info_bits
    }

    /// Retrieve the nth field from the tuple.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the field to retrieve.
    ///
    /// # Returns
    ///
    /// * The nth field of the tuple.
    pub fn dtuple_get_nth_field(&mut self, n: usize) -> &mut DField {
        info!("Getting field number: {}", n);
        if n < self.n_fields {
            self.fields.get_mut(n).unwrap()
        } else {
            panic!("Field number out of range");
        }
    }

    /// Gets the number of fields in the tuple.
    ///
    /// # Returns
    ///     
    /// * The number of fields in the tuple.
    ///
    pub fn dtuple_get_n_fields(&self) -> usize {
        info!("Getting number of fields: {}", self.n_fields);
        self.n_fields
    }

    /// Return the sum of data lengths of a tuple.
    /// The space occupied by tuple struct or field structs is not counted.
    ///
    /// # Returns
    ///
    /// * The sum of data lengths of the tuple.
    pub fn dtuple_get_data_size(&self) -> usize {
        let mut sum = 0;

        let n_fields = self.n_fields;
        for i in 0..n_fields {
            let field = self.fields.get(i).unwrap();
            let mut length = field.dfield_get_len();

            if length == UNIV_SQL_NULL {
                let dtype = field.dfield_get_type();
                length = dtype.dtype_get_sql_null_size();
            }

            sum += length;
        }

        sum
    }
}

impl DField {
    pub fn new(data_ptr: *mut u8) -> Self {
        DField {
            data: data_ptr,
            len: 0,
            ext: false,
            field_type: DType {
                m_type: 0,
                len: 0,
                pr_type: 0,
                mb_min_len: 0,
                mb_max_len: 0,
            },
        }
    }

    /// Set the data of the field.
    ///
    /// # Arguments
    ///
    /// * `data` - pointer to the data.
    /// * `len` - length of the data.
    ///
    pub fn dfield_set_data(&mut self, data: *const u8, len: usize) {
        info!("Setting data: {:?}", data);
        self.data = data;
        self.len = len;
    }

    /// Get the pointer to the data field.
    ///
    /// # Returns
    ///
    /// * Pointer to the data field.
    pub fn dfield_get_data(&self) -> *const u8 {
        info!("Getting data: {:?}", self.data);
        self.data
    }

    /// Set the type of the field.
    ///
    /// # Arguments
    ///
    /// * `m_type` - main data type.
    /// * `pr_type` - precision type.
    /// * `len` - precision of type.
    ///
    pub fn set_type(&mut self, m_type: usize, pr_type: usize, len: usize) {
        info!("Setting type: {} {} {}", m_type, pr_type, len);
        self.field_type.m_type = m_type as u8;
        self.field_type.pr_type = pr_type;
        self.field_type.len = len as u16;
    }

    /// Get length of field data.
    ///
    /// # Returns
    ///
    /// * The length of the field data.
    pub fn dfield_get_len(&self) -> usize {
        self.len
    }

    pub fn dfield_get_type(&self) -> DType {
        self.field_type.clone()
    }

    /// Determine if the field is an SQL nullptr.
    pub fn dfield_is_null(&self) -> bool {
        self.len == UNIV_SQL_NULL
    }
}

impl DType {
    pub fn dtype_get_sql_null_size(&self) -> usize {
        // The size of SQL NULL is 0
        0
    }
}
