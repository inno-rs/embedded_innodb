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
                FieldType::StringNullable,
            ],
        );

        let dtuple = dtuples.first().unwrap();

        assert_eq!(3, dtuple.n_fields);
        assert_eq!(3, dtuple.fields.len());

        let null_field = dtuple.dtuple_get_nth_field(2);
        assert!(null_field.dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_int() {
        let dtuples = create_dtuples(1, vec![FieldType::Int(Some(5))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 4);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_nullable_int() {
        let dtuples = create_dtuples(1, vec![FieldType::IntNullable(Some(5))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 4);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_null_int() {
        let dtuples = create_dtuples(1, vec![FieldType::IntNullable(None)]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 0);
        assert!(dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_string() {
        let dtuples = create_dtuples(
            1,
            vec![FieldType::String(Some("hello world!".to_string()))],
        );
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 12);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    #[ignore]
    fn test_dtuple_get_data_size_nullable_string() {}

    #[test]
    fn test_dtuple_get_data_size_null_string() {
        let dtuples = create_dtuples(1, vec![FieldType::StringNullable]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 0);
        assert!(dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_float() {
        let dtuples = create_dtuples(1, vec![FieldType::Float(Some(5.0))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 4);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_nullable_float() {
        let dtuples =
            create_dtuples(1, vec![FieldType::FloatNullable(Some(5.0))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 4);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_null_float() {
        let dtuples = create_dtuples(1, vec![FieldType::FloatNullable(None)]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 0);
        assert!(dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_double() {
        let dtuples = create_dtuples(1, vec![FieldType::Double(Some(5.0))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 8);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_nullable_double() {
        let dtuples =
            create_dtuples(1, vec![FieldType::DoubleNullable(Some(5.0))]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 8);
        assert!(!dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }

    #[test]
    fn test_dtuple_get_data_size_null_double() {
        let dtuples = create_dtuples(1, vec![FieldType::DoubleNullable(None)]);
        let dtuple = dtuples.first().unwrap();

        assert_eq!(dtuple.dtuple_get_data_size(), 0);
        assert!(dtuple.dtuple_get_nth_field(0).dfield_is_null());
    }
}
