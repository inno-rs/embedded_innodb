#[cfg(test)]
mod tests {
    use std::alloc::{Layout, alloc, dealloc};
    use std::cmp::Ordering;
    use std::sync::{Arc, Mutex};

    use rstest::rstest;
    use tracing::info;

    use crate::dict::dict0types::{Column, Field, Index};
    use crate::innodb::innodb0types::{PageId, UNIV_PAGE_SIZE};
    use crate::page::page0page::Page;
    use crate::rem::rem0cmp::cmp_rec_rec;
    use crate::rem::rem0rec::*;
    use crate::some_or_panic;
    use crate::tests::test0utils::{
        FieldType, create_new_fields, create_records, get_user_records,
    };

    fn get_new_rec() -> (Record, Layout, *mut u8) {
        let layout =
            Layout::from_size_align(REC_N_EXTRA_BYTES, UNIV_PAGE_SIZE)
                .unwrap();
        let ptr = unsafe { alloc(layout) };

        unsafe {
            let ptr: *mut u8 = ptr.add(REC_N_EXTRA_BYTES);
            (Record::new(ptr), layout, ptr)
        }
    }

    fn free_rec(layout: Layout, ptr: *mut u8) {
        unsafe {
            // Adjust the pointer back to the original allocation point
            let original_ptr = ptr.sub(REC_N_EXTRA_BYTES);
            dealloc(original_ptr, layout);
        }
    }

    #[test]
    fn test_page_align() {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        let p = unsafe { alloc(layout) };
        let rec_ptr = unsafe { p.add(897) };
        let rec = Record::new(rec_ptr);
        let aligned_ptr = rec.page_align();
        assert_eq!(aligned_ptr as usize % UNIV_PAGE_SIZE, 0);

        // Clean up
        unsafe {
            dealloc(p, layout);
        }
    }

    #[test]
    fn test_rec_set_n_fields() {
        let (rec, layout, ptr) = get_new_rec();

        for n_fields_val in (0..0xF).step_by(23) {
            info!("Setting n_fields to: {}", n_fields_val);
            info!("previous n_fields: {}", rec.rec_get_n_fields());

            rec.rec_set_n_fields(n_fields_val);
            let result = rec.rec_get_n_fields();

            assert_eq!(n_fields_val, result);
        }

        // Clean up
        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_set_next_offs() {
        let (rec, layout, p) = get_new_rec();

        for next_offset_val in (0..0xFFFF).step_by(3453) {
            info!("previous next_offset: {}", rec.rec_get_next_offs());

            rec.rec_set_next_offs(next_offset_val);
            let result = rec.rec_get_next_offs();

            assert_eq!(next_offset_val, result);
        }

        // Clean up
        free_rec(layout, p);
    }

    #[test]
    fn test_rec_set_info_bits() {
        let (rec, layout, ptr) = get_new_rec();

        for info_bits_val in 0..0xF {
            info!("previous info_bits: {}", rec.rec_get_info_bits());

            rec.rec_set_info_bits(info_bits_val);
            let result = rec.rec_get_info_bits();
            assert_eq!(info_bits_val, result);
        }

        // Clean up
        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_set_n_owned() {
        let (rec, layout, ptr) = get_new_rec();

        for n_owned_val in 0..0xF {
            info!("previous n_owned: {}", rec.rec_get_n_owned());

            rec.rec_set_n_owned(n_owned_val);
            let result = rec.rec_get_n_owned();
            assert_eq!(n_owned_val, result);
        }

        // Clean up
        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_set_heap_no() {
        let (rec, layout, ptr) = get_new_rec();

        for heap_no_val in (0..8191).step_by(19) {
            info!("previous heap_no: {}", rec.rec_get_heap_no());

            rec.rec_set_heap_no(heap_no_val);
            let result = rec.rec_get_heap_no();
            assert_eq!(heap_no_val, result);
        }

        // Clean up
        free_rec(layout, ptr);
    }

    #[test]
    fn test_linked_list_of_empty_page() {
        let owning_page = create_records(
            0,
            vec![
                FieldType::String(Some("Hello World 5".to_string())),
                FieldType::StringNull,
            ],
        );
        let page = owning_page.page;

        assert_eq!(page.page_n_get_recs(), 0);

        let infimum_rec = page.page_get_infimum_rec();
        let next_rec = infimum_rec.rec_get_next().unwrap();

        // There are no more elements after supremum.
        assert!(next_rec.rec_get_next().is_none());

        let supremum_rec = page.page_get_supremum_rec();
        assert!(supremum_rec.rec_get_next().is_none());

        assert_eq!(next_rec, supremum_rec);
    }

    #[test]
    fn test_linked_list_of_multiple_records() {
        let owning_page = create_records(
            10,
            vec![FieldType::String(Some("Hello World 4".to_string()))],
        );
        let page = owning_page.page;

        assert_eq!(page.page_n_get_recs(), 10);

        let mut count = 0;
        let mut cur_rec = Some(page.page_get_infimum_rec());
        assert!(page.page_rec_is_infimum(some_or_panic!(cur_rec)));
        while let Some(rec) = cur_rec {
            count += 1;
            cur_rec = some_or_panic!(cur_rec).rec_get_next();
        }

        // 10 + 2 (infimum & supremum)
        assert_eq!(count, 12);
    }

    #[test]
    fn test_rec_copy() {
        // Create two records
        let (source_rec, source_layout, source_ptr) = get_new_rec();
        let (mut dest_rec, dest_layout, dest_ptr) = get_new_rec();

        // Set values in the source record
        let n_fields_val = 0;
        let info_bits_val = 3;
        let heap_no_val = 42;
        let n_owned_val = 7;
        let next_offs_val = 1234;

        source_rec.rec_set_n_fields(n_fields_val);
        source_rec.rec_set_info_bits(info_bits_val);
        source_rec.rec_set_heap_no(heap_no_val);
        source_rec.rec_set_n_owned(n_owned_val);
        source_rec.rec_set_next_offs(next_offs_val);

        // Copy the source record to the destination record
        source_rec.rec_copy(&mut dest_rec);

        // Verify that the destination record has the same values as the source record
        assert_eq!(dest_rec.rec_get_n_fields(), n_fields_val);
        assert_eq!(dest_rec.rec_get_info_bits(), info_bits_val);
        assert_eq!(dest_rec.rec_get_heap_no(), heap_no_val);
        assert_eq!(dest_rec.rec_get_n_owned(), n_owned_val);
        assert_eq!(dest_rec.rec_get_next_offs(), next_offs_val);

        // Clean up
        free_rec(source_layout, source_ptr);
        free_rec(dest_layout, dest_ptr);
    }

    #[test]
    fn test_rec_field_external_short_format() {
        let owning_page = create_records(
            10,
            vec![FieldType::String(Some("Hello World 3".to_string()))],
        );
        let page = owning_page.page;

        for rec in get_user_records(page) {
            if page.page_rec_is_user_rec(rec) {
                assert!(rec.rec_get_1byte_offs_flag());
            }
        }
    }

    #[ignore]
    #[test]
    fn test_rec_field_external_2byte_format() {}

    #[test]
    fn test_rec_field_external_no_field_external() {
        let owning_page = create_records(
            10,
            vec![FieldType::String(Some("Hello World 2".to_string()))],
        );
        let page = owning_page.page;

        for rec in get_user_records(page) {
            if page.page_rec_is_user_rec(rec) {
                let n_fields = rec.rec_get_n_fields();
                for i in 0..n_fields {
                    assert!(!rec.rec_nth_external(i));
                }
            }
        }
    }

    #[ignore]
    #[test]
    fn test_rec_field_external_many_fields_external() {}

    #[test]
    fn test_rec_needs_external() {
        assert_eq!(Page::page_rec_needs_ext(100), false);
        assert_eq!(Page::page_rec_needs_ext(100000), true);
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
        let user_records_1 = get_user_records(owning_page_1.page);
        let rec1 = user_records_1[0];

        let owning_page_2 =
            create_records(1, vec![FieldType::String(Some(str2.to_string()))]);
        let user_records_2 = get_user_records(owning_page_2.page);
        let rec2 = user_records_2[0];

        let mut index = Index::new(PageId { space_id: 0, page_no: 0 });
        let fields = create_new_fields(1);
        for field in fields {
            index.add_field(field);
        }

        info!("rec: {:?}", rec1.rec_to_string());
        info!("rec: {:?}", rec2.rec_to_string());

        let result = cmp_rec_rec(rec1, rec2, &index);

        assert_eq!(result, out);
    }
}
