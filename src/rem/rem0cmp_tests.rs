#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tracing::{debug, info};

    use crate::{
        dict::dict0types::Index,
        innodb::innodb0types::PageId,
        page::page0page::PageDebug,
        rem::rem0cmp::{cmp_dtuple_rec_with_match, cmp_rec_rec},
        tests::test0utils::{
            FieldType, create_dtuples, create_new_fields, create_records,
        },
    };

    #[derive(Debug)]
    struct ComparisonInput<const N: usize> {
        pub left: [FieldType; N],
        pub right: [FieldType; N],
        pub expected: Ordering,
    }

    #[rstest]
    #[case("", "", Ordering::Equal)]
    #[case("", "a", Ordering::Less)]
    #[case("a", "", Ordering::Greater)]
    #[case("a", "a", Ordering::Equal)]
    #[case("a", "b", Ordering::Less)]
    #[case("b", "a", Ordering::Greater)]
    #[case("abc", "abc", Ordering::Equal)]
    #[case("abc", "abd", Ordering::Less)]
    #[case("abd", "abc", Ordering::Greater)]
    #[case("abc", "abcd", Ordering::Less)] // prefix
    #[case("abcd", "abc", Ordering::Greater)]
    #[case("abc1", "abc2", Ordering::Less)] // numeric tail
    #[case("abc2", "abc1", Ordering::Greater)]
    #[case("abc", "ABC", Ordering::Greater)] // ASCII case: lowercase > uppercase
    #[case("ABC", "abc", Ordering::Less)]
    #[case("abc", "abC", Ordering::Greater)] // mixed case
    #[case("abC", "abc", Ordering::Less)]
    #[case("abc", "abc ", Ordering::Less)] // trailing space
    #[case("abc ", "abc", Ordering::Greater)]
    #[case("abc", "abc\n", Ordering::Less)] // newline
    #[case("abc\n", "abc", Ordering::Greater)]
    #[case("123", "123", Ordering::Equal)] // numeric strings
    #[case("123", "124", Ordering::Less)]
    #[case("124", "123", Ordering::Greater)]
    #[case("🙂", "🙂", Ordering::Equal)] // Unicode emoji
    #[case("🙂", "🙃", Ordering::Less)]
    #[case("🙃", "🙂", Ordering::Greater)]
    #[case("Z", "a", Ordering::Less)] // Uppercase vs lowercase farther apart
    #[case("a", "Z", Ordering::Greater)]
    fn test_rec_rec_match(
        #[case] str1: &str,
        #[case] str2: &str,
        #[case] out: Ordering,
    ) {
        info!("case: 1={}, 2={}", str1, str2);
        let owning_page_1 =
            create_records(1, vec![FieldType::String(Some(str1.to_string()))]);
        let user_records_1 = owning_page_1.page.page_get_user_records();
        let rec1 = user_records_1[0];

        let owning_page_2 =
            create_records(1, vec![FieldType::String(Some(str2.to_string()))]);
        let user_records_2 = owning_page_2.page.page_get_user_records();
        let rec2 = user_records_2[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(1);
        for field in fields {
            index.add_field(field);
        }

        let result = cmp_rec_rec(rec1, rec2, &index);

        assert_eq!(result, out);
    }

    #[rstest]
    #[case(vec![s("hello"), s("world")], vec![s("hello"), s("world")], Ordering::Equal)]
    #[case(vec![s("a"), s("z")],vec![s("b"), s("z")],Ordering::Less)]
    // #[case(vec![s("a"), s("z")],vec![s("a"), s("zz")],Ordering::Less)]
    // #[case(vec![s("abc"), s("def")],vec![s("abc"), s("defg")],Ordering::Less)]
    #[case(vec![s("abc"), s("def")],vec![s("abc"), s("abc")],Ordering::Greater
    )]
    #[case(vec![s("abc"), s("abc")],vec![s("abc"), s("abc")],Ordering::Equal)]
    #[case(vec![s("abc"), s("abc")],vec![s("abc"), s("zzz")],Ordering::Less)]
    #[case(vec![s(""), s("a")],vec![s(""), s("b")],Ordering::Less)]
    #[case(vec![s("a"), s("")],vec![s("a"), s("")],Ordering::Equal)]
    #[case(vec![s("Hello"), s("World")],vec![s("hello"), s("world")],Ordering::Less // because "H" < "h" in ASCII
    )]
    #[case(vec![s("a"), s("b")],vec![s("a"), s("b")],Ordering::Equal)]
    #[case(vec![s("a"), s("b")],vec![s("a"), s("c")],Ordering::Less)]
    #[case(vec![s("a"), s("c")],vec![s("a"), s("b")],Ordering::Greater)]
    #[case(vec![FieldType::StringNullable, s("a")],vec![s("a"), s("a")],Ordering::Less)]
    #[case(vec![s("a"), FieldType::StringNullable],vec![s("a"), s("a")],Ordering::Less)]
    // #[case(vec![FieldType::StringNullable, FieldType::StringNullable],vec![FieldType::StringNullable, FieldType::StringNullable],Ordering::Equal
    // )]
    #[case(vec![s("a"), s("a")],vec![FieldType::StringNullable, s("a")],Ordering::Greater)]
    #[case(vec![s(""), s("a")],vec![s(""), s("a")],Ordering::Equal)]
    fn test_rec_rec_match_with_multiple_string_fields(
        #[case] fields1: Vec<FieldType>,
        #[case] fields2: Vec<FieldType>,
        #[case] out: Ordering,
    ) {
        let owning_page_1 = create_records(1, fields1);
        let user_records_1 = owning_page_1.page.page_get_user_records();
        let rec1 = user_records_1[0];

        let owning_page_2 = create_records(1, fields2);
        let user_records_2 = owning_page_2.page.page_get_user_records();
        let rec2 = user_records_2[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(8);
        for field in fields {
            index.add_field(field);
        }

        debug!("rec1={}", rec1);
        debug!("rec2={}", rec2);

        let result = cmp_rec_rec(rec1, rec2, &index);

        assert_eq!(result, out);
    }

    #[rstest]
    #[case(vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Equal
    )]
    #[case(vec![FieldType::Int(Some(5)), FieldType::Int(Some(100))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(100))], Ordering::Less
    )]
    #[case(vec![FieldType::Int(Some(15)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(15)), FieldType::Int(Some(10))], Ordering::Greater
    )]
    #[case(vec![FieldType::Int(Some(0)), FieldType::Int(Some(0))], vec![FieldType::Int(Some(0)), FieldType::Int(Some(1))], Ordering::Less
    )]
    #[case(vec![FieldType::IntNullable(None), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Less
    )]
    #[case(vec![FieldType::Int(Some(10)), FieldType::IntNullable(None)], vec![FieldType::Int(Some(10)), FieldType::Int(Some(10))], Ordering::Less
    )]
    // #[case(vec![FieldType::IntNullable(None), FieldType::IntNullable(None)], vec![FieldType::IntNullable(None), FieldType::IntNullable(None)], Ordering::Equal
    // )]
    #[case(vec![FieldType::Int(Some(42)), FieldType::Int(Some(0))], vec![FieldType::Int(Some(42)), FieldType::Int(Some(1))], Ordering::Less
    )]
    fn test_rec_rec_match_with_multiple_int_fields(
        #[case] fields1: Vec<FieldType>,
        #[case] fields2: Vec<FieldType>,
        #[case] out: Ordering,
    ) {
        let owning_page_1 = create_records(1, fields1);
        let user_records_1 = owning_page_1.page.page_get_user_records();
        let rec1 = user_records_1[0];

        let owning_page_2 = create_records(1, fields2);
        let user_records_2 = owning_page_2.page.page_get_user_records();
        let rec2 = user_records_2[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(8);
        for field in fields {
            index.add_field(field);
        }

        let result = cmp_rec_rec(rec1, rec2, &index);

        assert_eq!(result, out);
    }

    #[rstest]
    #[case(vec![s("hello"), s("world")], vec![s("hello"), s("world")], Ordering::Equal)]
    #[case(vec![s("a"), s("z")],vec![s("b"), s("z")],Ordering::Less)]
    // #[case(vec![s("a"), s("z")],vec![s("a"), s("zz")],Ordering::Less)]
    // #[case(vec![s("abc"), s("def")],vec![s("abc"), s("defg")],Ordering::Less)]
    #[case(vec![s("abc"), s("def")],vec![s("abc"), s("abc")],Ordering::Greater
    )]
    #[case(vec![s("abc"), s("abc")],vec![s("abc"), s("abc")],Ordering::Equal)]
    #[case(vec![s("abc"), s("abc")],vec![s("abc"), s("zzz")],Ordering::Less)]
    #[case(vec![s(""), s("a")],vec![s(""), s("b")],Ordering::Less)]
    // #[case(vec![s("a"), s("")],vec![s("a"), s("")],Ordering::Equal)]
    #[case(vec![s("Hello"), s("World")],vec![s("hello"), s("world")],Ordering::Less // because "H" < "h" in ASCII
    )]
    #[case(vec![s("a"), s("b")],vec![s("a"), s("b")],Ordering::Equal)]
    #[case(vec![s("a"), s("b")],vec![s("a"), s("c")],Ordering::Less)]
    #[case(vec![s("a"), s("c")],vec![s("a"), s("b")],Ordering::Greater)]
    #[case(vec![FieldType::StringNullable, s("a")],vec![s("a"), s("a")],Ordering::Less)]
    // #[case(vec![s("a"), FieldType::StringNull],vec![s("a"), s("a")],Ordering::Less)]
    // #[case(vec![FieldType::StringNull, FieldType::StringNull],vec![FieldType::StringNull, FieldType::StringNull],Ordering::Equal
    // )]
    #[case(vec![s("a"), s("a")],vec![FieldType::StringNullable, s("a")],Ordering::Greater)]
    #[case(vec![s(""), s("a")],vec![s(""), s("a")],Ordering::Equal)]
    fn test_cmp_dtuple_rec_match_with_multiple_string_fields(
        #[case] fields1: Vec<FieldType>,
        #[case] fields2: Vec<FieldType>,
        #[case] out: Ordering,
    ) {
        let dtuples = create_dtuples(1, fields1);
        let dtuple = &dtuples[0];

        let owning_page = create_records(1, fields2);
        let user_records = owning_page.page.page_get_user_records();
        let rec2 = user_records[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(8);
        for field in fields {
            index.add_field(field);
        }

        let mut matched_fields = 0;
        let mut matched_bytes = 0;
        let result = cmp_dtuple_rec_with_match(
            dtuple,
            rec2,
            &mut matched_fields,
            &mut matched_bytes,
        );

        assert_eq!(result, out);
    }

    #[rstest]
    #[case(vec![FieldType::Int(Some(i32::MAX)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Greater
    )]
    #[case(vec![FieldType::Int(Some(i32::MAX)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(i32::MAX)), FieldType::Int(Some(i32::MAX))], Ordering::Less
    )]
    #[case(vec![FieldType::Int(Some(312910472)), FieldType::Int(Some(170239484))], vec![FieldType::Int(Some(289174102)), FieldType::Int(Some(754120931))], Ordering::Greater
    )]
    #[case(vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Equal
    )]
    #[case(vec![FieldType::Int(Some(i32::MAX)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Greater
    )]
    #[case(vec![FieldType::Int(Some(5)), FieldType::Int(Some(100))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(100))], Ordering::Less
    )]
    #[case(vec![FieldType::Int(Some(15)), FieldType::Int(Some(20))], vec![FieldType::Int(Some(15)), FieldType::Int(Some(10))], Ordering::Greater
    )]
    #[case(vec![FieldType::Int(Some(0)), FieldType::Int(Some(0))], vec![FieldType::Int(Some(0)), FieldType::Int(Some(1))], Ordering::Less
    )]
    #[case(vec![FieldType::IntNullable(None), FieldType::Int(Some(20))], vec![FieldType::Int(Some(10)), FieldType::Int(Some(20))], Ordering::Less
    )]
    // todo: Handle nulls
    // #[case(vec![FieldType::Int(Some(10)), FieldType::IntNullable(None)], vec![FieldType::Int(Some(10)), FieldType::Int(Some(10))], Ordering::Less
    // )]
    #[case(vec![FieldType::Int(Some(10)), FieldType::Int(Some(10))], vec![FieldType::Int(Some(10)), FieldType::IntNullable(None)], Ordering::Greater
    )]
    #[case(vec![FieldType::IntNullable(None), FieldType::IntNullable(None)], vec![FieldType::IntNullable(None), FieldType::IntNullable(None)], Ordering::Equal
    )]
    #[case(vec![FieldType::Int(Some(42)), FieldType::Int(Some(0))], vec![FieldType::Int(Some(42)), FieldType::Int(Some(1))], Ordering::Less
    )]
    fn test_cmp_dtuple_rec_match_with_multiple_int_fields(
        #[case] fields1: Vec<FieldType>,
        #[case] fields2: Vec<FieldType>,
        #[case] out: Ordering,
    ) {
        let dtuples = create_dtuples(1, fields1);
        let dtuple = &dtuples[0];

        let owning_page = create_records(1, fields2);
        let user_records = owning_page.page.page_get_user_records();
        let rec2 = user_records[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(8);
        for field in fields {
            index.add_field(field);
        }

        let mut matched_fields = 0;
        let mut matched_bytes = 0;
        let result = cmp_dtuple_rec_with_match(
            dtuple,
            rec2,
            &mut matched_fields,
            &mut matched_bytes,
        );

        assert_eq!(result, out);
    }

    #[test]
    fn test_cmp_dtuple_rec_match_with_multiple_float_fields() {
        let input = Vec::from([
            ComparisonInput {
                left: [
                    FieldType::Float(Some(1.0e30)),
                    FieldType::Float(Some(2.0)),
                ],
                right: [
                    FieldType::Float(Some(1.0e30)),
                    FieldType::Float(Some(2.0)),
                ],
                expected: Ordering::Equal,
            },
            ComparisonInput {
                left: [
                    FieldType::Float(Some(1.0e38)), // near max f32
                    FieldType::Float(Some(0.0)),
                ],
                right: [
                    FieldType::Float(Some(1.0e37)),
                    FieldType::Float(Some(0.0)),
                ],
                expected: Ordering::Greater,
            },
            ComparisonInput {
                left: [
                    FieldType::Float(Some(-1.0e30)),
                    FieldType::Float(Some(5.0)),
                ],
                right: [
                    FieldType::Float(Some(-1.0e30)),
                    FieldType::Float(Some(10.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::Float(Some(1.0e-30)),
                    FieldType::Float(Some(0.0)),
                ],
                right: [
                    FieldType::Float(Some(1.0e-30)),
                    FieldType::Float(Some(0.1)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::Float(Some(0.0)),
                    FieldType::Float(Some(0.0)),
                ],
                right: [
                    FieldType::Float(Some(0.0)),
                    FieldType::Float(Some(-1.0e-38)),
                ],
                expected: Ordering::Greater,
            },
            ComparisonInput {
                left: [
                    FieldType::Float(Some(-3.4e38)), // near min f32
                    FieldType::Float(Some(0.0)),
                ],
                right: [
                    FieldType::Float(Some(-3.0e38)),
                    FieldType::Float(Some(0.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::FloatNullable(None),
                    FieldType::Float(Some(10.0)),
                ],
                right: [
                    FieldType::Float(Some(1.0e5)),
                    FieldType::Float(Some(10.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::FloatNullable(None),
                    FieldType::FloatNullable(None),
                ],
                right: [
                    FieldType::FloatNullable(None),
                    FieldType::FloatNullable(None),
                ],
                expected: Ordering::Equal,
            },
        ]);

        for comp_input in input {
            let dtuples = create_dtuples(1, comp_input.left.to_vec());
            let dtuple = &dtuples[0];

            let owning_page = create_records(1, comp_input.right.to_vec());
            let user_records = owning_page.page.page_get_user_records();
            let rec2 = user_records[0];

            let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
            let fields = create_new_fields(8);
            for field in fields {
                index.add_field(field);
            }

            let mut matched_fields = 0;
            let mut matched_bytes = 0;
            let result = cmp_dtuple_rec_with_match(
                dtuple,
                rec2,
                &mut matched_fields,
                &mut matched_bytes,
            );

            assert_eq!(
                result, comp_input.expected,
                "Failed for input: {:?}",
                comp_input
            );
        }
    }

    #[test]
    fn test_cmp_dtuple_rec_match_with_multiple_double_fields() {
        let input = Vec::from([
            ComparisonInput {
                left: [
                    FieldType::Double(Some(1.0e300)),
                    FieldType::Double(Some(2.0)),
                ],
                right: [
                    FieldType::Double(Some(1.0e300)),
                    FieldType::Double(Some(2.0)),
                ],
                expected: Ordering::Equal,
            },
            ComparisonInput {
                left: [
                    FieldType::Double(Some(1.0e308)), // near max f64
                    FieldType::Double(Some(0.0)),
                ],
                right: [
                    FieldType::Double(Some(1.0e307)),
                    FieldType::Double(Some(0.0)),
                ],
                expected: Ordering::Greater,
            },
            ComparisonInput {
                left: [
                    FieldType::Double(Some(-1.0e300)),
                    FieldType::Double(Some(5.0)),
                ],
                right: [
                    FieldType::Double(Some(-1.0e300)),
                    FieldType::Double(Some(10.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::Double(Some(1.0e-300)),
                    FieldType::Double(Some(0.0)),
                ],
                right: [
                    FieldType::Double(Some(1.0e-300)),
                    FieldType::Double(Some(0.1)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::Double(Some(0.0)),
                    FieldType::Double(Some(0.0)),
                ],
                right: [
                    FieldType::Double(Some(0.0)),
                    FieldType::Double(Some(-1.0e-308)),
                ],
                expected: Ordering::Greater,
            },
            ComparisonInput {
                left: [
                    FieldType::Double(Some(-1.7e308)), // near min f64
                    FieldType::Double(Some(0.0)),
                ],
                right: [
                    FieldType::Double(Some(-1.5e308)),
                    FieldType::Double(Some(0.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::DoubleNullable(None),
                    FieldType::Double(Some(10.0)),
                ],
                right: [
                    FieldType::Double(Some(1.0e5)),
                    FieldType::Double(Some(10.0)),
                ],
                expected: Ordering::Less,
            },
            ComparisonInput {
                left: [
                    FieldType::DoubleNullable(None),
                    FieldType::DoubleNullable(None),
                ],
                right: [
                    FieldType::DoubleNullable(None),
                    FieldType::DoubleNullable(None),
                ],
                expected: Ordering::Equal,
            },
        ]);

        for comp_input in input {
            let dtuples = create_dtuples(1, comp_input.left.to_vec());
            let dtuple = &dtuples[0];

            let owning_page = create_records(1, comp_input.right.to_vec());
            let user_records = owning_page.page.page_get_user_records();
            let rec2 = user_records[0];

            let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
            let fields = create_new_fields(8);
            for field in fields {
                index.add_field(field);
            }

            let mut matched_fields = 0;
            let mut matched_bytes = 0;
            let result = cmp_dtuple_rec_with_match(
                dtuple,
                rec2,
                &mut matched_fields,
                &mut matched_bytes,
            );

            assert_eq!(
                result, comp_input.expected,
                "Failed for input: {:?}",
                comp_input
            );
        }
    }

    fn s(value: &str) -> FieldType {
        FieldType::String(Some(value.to_string()))
    }
}
