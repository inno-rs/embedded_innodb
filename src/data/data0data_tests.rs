#[cfg(test)]
mod tests {
    use crate::tests::test0utils::{FieldType, create_dtuples};

    #[test]
    fn test_create_data_tuple() {
        let dtuples = create_dtuples(
            1,
            vec![
                FieldType::String(Some("heloo 1".to_string())),
                FieldType::String(Some("heloo 2".to_string())),
                FieldType::StringNull,
            ],
        );

        let dtuple = dtuples.first().unwrap();

        assert_eq!(3, dtuple.n_fields);
        assert_eq!(3, dtuple.fields.len());

        let null_field = dtuple.dtuple_get_nth_field(2);
        assert!(null_field.dfield_is_null());
    }
}
