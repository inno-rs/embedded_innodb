#[cfg(test)]
mod tests {
    use crate::data::data0types::{DField, DTupleBuilder};

    #[test]
    fn test_create_data_tuple() {
        let mut field1_data = vec![0u8; 10];
        let mut field2_data = vec![0u8; 10];
        let dtuple_result = DTupleBuilder::default()
            .info_bits(1)
            .add_field(DField::new(field1_data.as_mut_ptr()))
            .add_field(DField::new(field2_data.as_mut_ptr()))
            .n_fields(2)
            .build();

        let dtuple = dtuple_result.unwrap();

        assert_eq!(2, dtuple.n_fields);
        assert_eq!(2, dtuple.fields.len());
    }
}
