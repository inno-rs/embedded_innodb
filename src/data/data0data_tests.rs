#[cfg(test)]
mod tests {
    use crate::data::data0types::{
        DFieldBuilder, DTupleBuilder, DTypeBuilder,
    };
    use crate::innodb::innodb0types::UNIV_SQL_NULL;

    #[test]
    fn test_create_data_tuple() {
        let mut field1_data = vec![0u8; 10];
        let mut field2_data = vec![0u8; 10];
        let mut field3_data = vec![0u8; 10];
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

        let mut dtuple = dtuple_result.unwrap();

        assert_eq!(3, dtuple.n_fields);
        assert_eq!(3, dtuple.fields.len());

        let null_field = dtuple.dtuple_get_nth_field(2);
        assert!(null_field.dfield_is_null());
    }
}
