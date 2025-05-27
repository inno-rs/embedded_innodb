use crate::data::data0types::{
    BigRec, DField, DTuple, DType, DATA_CHAR, DATA_CLIENT, DATA_INT,
    DATA_VARCHAR,
};
use crate::innodb::innodb0types::UNIV_SQL_NULL;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use log::{error, info};

impl DTuple {
    /// Creates a new tuple with the specified number of fields.
    ///
    /// # Arguments
    ///
    /// * `number_of_fields` - The number of fields to allocate in the tuple.
    ///
    /// # Returns
    ///
    /// * A new DTuple instance with the specified number of fields.
    pub fn new(number_of_fields: usize) -> Self {
        let d_fields = vec![DField::new(vec![]); number_of_fields];
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

    /// Gets the info bits of the tuple.
    ///
    /// # Returns
    ///
    /// * The info bits of the tuple.
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
    pub fn dtuple_get_nth_field_mut(&mut self, n: usize) -> &mut DField {
        info!("Getting field number: {}", n);
        if n < self.n_fields {
            self.fields.get_mut(n).unwrap()
        } else {
            panic!("Field number out of range");
        }
    }

    /// Retrieve the nth field from the tuple (immutable reference).
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the field to retrieve.
    ///
    /// # Returns
    ///
    /// * The nth field of the tuple.
    ///
    /// # Panics
    ///
    /// * If the field number is out of range.
    pub fn dtuple_get_nth_field(&self, n: usize) -> &DField {
        info!("Getting field number: {}", n);
        if n < self.n_fields {
            self.fields.get(n).unwrap()
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

    /// Return the size of data tuple when converted to physical record.
    pub fn rec_get_converted_size(&self) -> usize {
        self.dtuple_get_data_size()
    }

    pub fn dtuple_validate(&self) -> bool {
        ut_a!(self.dtuple_type_checked());
        true
    }

    pub fn dtuple_convert_big_rec(&mut self, n_ext: usize) -> Option<BigRec> {
        Some(BigRec { n_fields: 0, fields: vec![] })
    }

    pub fn dtuple_type_checked(&self) -> bool {
        let n_fields = self.dtuple_get_n_fields();
        for index in 0..n_fields {
            let field = self.dtuple_get_nth_field(index);

            ut_a!(field.dfield_check_typed());
        }

        true
    }
}

impl DField {
    /// Creates a new data field with the specified data pointer.
    ///
    /// # Arguments
    ///
    /// * `data_ptr` - Pointer to the data for this field.
    ///
    /// # Returns
    ///
    /// * A new DField instance with the specified data pointer.
    pub fn new(data: Vec<u8>) -> Self {
        DField {
            data,
            len: 0,
            ext: false,
            dtype: DType {
                m_type: 0,
                len: 0,
                pr_type: 0,
                mb_min_len: 0,
                mb_max_len: 0,
            },
        }
    }

    pub fn dfield_set_null(&mut self) {
        self.dfield_set_data(vec![], UNIV_SQL_NULL);
    }

    /// Set the data of the field.
    ///
    /// # Arguments
    ///
    /// * `data` - pointer to the data.
    /// * `len` - length of the data.
    ///
    pub fn dfield_set_data(&mut self, data: Vec<u8>, len: usize) {
        info!("Setting data: {:?}", data);
        ut_ad!((data.len() == 0 && len == UNIV_SQL_NULL) || data.len() == len);

        self.data = data;
        self.len = len;
    }

    /// Get the pointer to the data field.
    ///
    /// # Returns
    ///
    /// * Pointer to the data field.
    pub fn dfield_get_data(&self) -> &Vec<u8> {
        info!("Getting data: {:?}", self.data);
        self.data.as_ref()
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
        self.dtype.m_type = m_type as u16;
        self.dtype.pr_type = pr_type;
        self.dtype.len = len as u16;
    }

    /// Get length of field data.
    ///
    /// # Returns
    ///
    /// * The length of the field data.
    pub fn dfield_get_len(&self) -> usize {
        self.len
    }

    /// Gets the data type of the field.
    ///
    /// # Returns
    ///
    /// * A clone of the field's data type.
    pub fn dfield_get_type(&self) -> DType {
        self.dtype.clone()
    }

    /// Determines if the field is an SQL nullptr.
    ///
    /// # Returns
    ///
    /// * `true` if the field is NULL, `false` otherwise.
    pub fn dfield_is_null(&self) -> bool {
        self.len == UNIV_SQL_NULL
    }

    /// Determines if the field is stored externally.
    ///
    /// # Returns
    ///
    /// * `true` if the field is stored externally, `false` otherwise.
    pub fn dfield_is_ext(&self) -> bool {
        self.ext
    }

    pub fn dfield_print(&self) {
        if self.dfield_is_null() {
            info!("NULL");
            return;
        }

        let data = self.dfield_get_data();

        let mut output: String = String::new();

        match self.dfield_get_type().m_type as usize {
            n if n == DATA_CHAR || n == DATA_VARCHAR => {
                output.push_str(format!("{:?}", data).as_str());

                if self.dfield_is_ext() {
                    output.push_str(format!("{}", "external").as_str());
                }
            }
            n if n == DATA_INT => {
                ut_a!(data.len() == 4);
                output.push_str(format!("{:?}", data).as_str());
            }
            _ => {}
        }

        info!("{:?}", output);
    }

    /// Checks if the data in the tuple is typed.
    pub fn dfield_check_typed(&self) -> bool {
        let dtype = self.dfield_get_type();
        if dtype.m_type > DATA_CLIENT as u16
            || dtype.m_type < DATA_VARCHAR as u16
        {
            error!(
                "Error: data type field {}, len {}",
                dtype.m_type, dtype.len
            );

            ut_ad!(false);
            return false;
        }

        true
    }

    /// Compares two data fields for binary equality.
    ///
    /// # Arguments
    ///
    /// * `field1` - The first field to compare.
    /// * `field2` - The second field to compare.
    ///
    /// # Returns
    ///
    /// * `true` if the fields have identical data, `false` otherwise.
    pub fn dfield_data_are_binary_equal(
        field1: &DField,
        field2: &DField,
    ) -> bool {
        let len = field1.dfield_get_len();
        len == field2.dfield_get_len()
            && (field1.dfield_get_len() == UNIV_SQL_NULL
                || field1.dfield_get_data().eq(field2.dfield_get_data()))
    }
}

impl DType {
    /// Gets the size of an SQL NULL value for this data type.
    ///
    /// # Returns
    ///
    /// * The size of an SQL NULL value for this data type.
    pub fn dtype_get_sql_null_size(&self) -> usize {
        // The size of SQL NULL is 0
        0
    }
}
