#[cfg(test)]
mod tests {
    use std::alloc::{Layout, alloc, dealloc};

    use pretty_assertions::assert_eq;
    use tracing::info;

    use crate::{
        innodb::innodb0types::UNIV_PAGE_SIZE,
        page::page0page::{Page, PageDebug},
        rem::rem0rec::*,
        some_or_panic,
        tests::test0utils::{FieldType, create_records},
        ut::ut0ut::random_string_with_seed,
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
                FieldType::StringNullable,
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

        for rec in page.page_get_user_records() {
            if page.page_rec_is_user_rec(rec) {
                assert!(rec.rec_get_1byte_offs_flag());
            }
        }
    }

    #[test]
    #[ignore]
    fn test_rec_field_external_2byte_format() {
        let owning_page = create_records(
            10,
            vec![FieldType::String(Some(random_string_with_seed(127, 42)))],
        );
        let page = owning_page.page;

        for rec in page.page_get_user_records() {
            if page.page_rec_is_user_rec(rec) {
                assert!(!rec.rec_get_1byte_offs_flag());
            }
        }
    }

    #[test]
    fn test_rec_get_converted_extra_size() {
        // No external field
        {
            let data_size = 10;
            let n_ext = 0;
            let n_fields = 10;
            let converted_size = Record::rec_get_converted_extra_size(
                data_size, n_fields, n_ext,
            );

            assert_eq!(converted_size, 16);
        }
        // 1 external field
        {
            let data_size = 10;
            let n_ext = 1;
            let n_fields = 10;
            let converted_size = Record::rec_get_converted_extra_size(
                data_size, n_fields, n_ext,
            );

            assert_eq!(converted_size, 26);
        }
        // 10 external fields
        {
            let data_size = 10;
            let n_ext = 10;
            let n_fields = 10;
            let converted_size = Record::rec_get_converted_extra_size(
                data_size, n_fields, n_ext,
            );

            assert_eq!(converted_size, 26);
        }
        // data size exceeding 1byte offset format
        {
            let data_size = REC_1BYTE_OFFS_LIMIT + 1;
            let n_ext = 0;
            let n_fields = 10;
            let converted_size = Record::rec_get_converted_extra_size(
                data_size, n_fields, n_ext,
            );

            assert_eq!(converted_size, 26);
        }

        // data size exceeding 1byte offset format and multiple external fields
        {
            let data_size = REC_1BYTE_OFFS_LIMIT + 1;
            let n_ext = 10;
            let n_fields = 10;
            let converted_size = Record::rec_get_converted_extra_size(
                data_size, n_fields, n_ext,
            );

            assert_eq!(converted_size, 26);
        }
    }

    #[test]
    fn test_rec_field_external_no_field_external() {
        let owning_page = create_records(
            10,
            vec![FieldType::String(Some("Hello World 2".to_string()))],
        );
        let page = owning_page.page;

        for rec in page.page_get_user_records() {
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

    #[test]
    fn test_record_field_data_is_sql_null() {
        let sql_null_data = RecordFieldData::SqlNullData();
        let string_data = RecordFieldData::Data("test".to_string());

        assert!(sql_null_data.is_sql_null());
        assert!(!string_data.is_sql_null());
    }

    #[test]
    fn test_record_field_data_or_panic() {
        let string_data = RecordFieldData::Data("test".to_string());
        assert_eq!(string_data.data_or_panic(), "test");

        // We can't test the panic case directly as it would crash the test
    }

    #[test]
    fn test_orphan_record_new_and_get_record() {
        let data = vec![0; 100];
        let n_fields = 5;
        let mut orphan_record = OrphanRecord::new(data, n_fields);

        let record = orphan_record.get_record();
        assert_eq!(record.rec_get_n_fields(), 0); // Default value before setting
    }

    #[test]
    fn test_record_new_and_get_ptr() {
        let (rec, layout, ptr) = get_new_rec();

        assert_eq!(rec.get_ptr() as *mut u8, ptr);

        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_get_prev() {
        let owning_page = create_records(
            3,
            vec![FieldType::String(Some("Test".to_string()))],
        );
        let page = owning_page.page;

        // Get the first user record
        let first_rec = page.page_get_user_records()[0];

        // Get the second user record
        let second_rec = first_rec.rec_get_next().unwrap();

        // The previous record of the second record should be the first record
        let prev_of_second = second_rec.rec_get_prev().unwrap();
        assert_eq!(prev_of_second, first_rec);
    }

    #[test]
    fn test_rec_get_data_size() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // The data size should be the sum of the field lengths
        // "Hello" (5) + "World" (5) = 10
        assert_eq!(rec.rec_get_data_size(), 10);
    }

    #[test]
    fn test_rec_get_extra_size() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // For 2 fields with 1-byte offsets, the extra size should be:
        // REC_N_EXTRA_BYTES (6) + n_fields (2) = 8
        assert_eq!(rec.rec_get_extra_size(), 8);
    }

    #[test]
    fn test_rec_get_nth_field() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // Get the first field
        let (offset, size) = rec.rec_get_nth_field(0);
        assert_eq!(size, 5); // "Hello" has 5 bytes

        // Get the second field
        let (offset, size) = rec.rec_get_nth_field(1);
        assert_eq!(size, 5); // "World" has 5 bytes
    }

    #[test]
    fn test_rec_get_data() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // Get the first field data
        let data = rec.rec_get_data(0).unwrap();
        match data {
            RecordFieldData::Data(s) => assert_eq!(s, "Hello"),
            RecordFieldData::SqlNullData() => {
                panic!("Expected string data, got SQL NULL")
            }
        }

        // Get the second field data
        let data = rec.rec_get_data(1).unwrap();
        match data {
            RecordFieldData::Data(s) => assert_eq!(s, "World"),
            RecordFieldData::SqlNullData() => {
                panic!("Expected string data, got SQL NULL")
            }
        }
    }

    #[test]
    fn test_rec_get_nth_field_size() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // Get the size of the first field
        let size = rec.rec_get_nth_field_size(0);
        assert_eq!(size, 5); // "Hello" has 5 bytes

        // Get the size of the second field
        let size = rec.rec_get_nth_field_size(1);
        assert_eq!(size, 5); // "World" has 5 bytes
    }

    #[test]
    fn test_rec_validate() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // The record should be valid
        assert!(rec.rec_validate());
    }

    #[test]
    fn test_get_nth_field_byte_at_offset() {
        let owning_page = create_records(
            1,
            vec![FieldType::String(Some("Hello".to_string()))],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // Get the first byte of the first field
        let byte = rec.get_nth_field_byte_at_offset(0, 0);
        assert_eq!(byte, b'H');

        // Get the second byte of the first field
        let byte = rec.get_nth_field_byte_at_offset(0, 1);
        assert_eq!(byte, b'e');
    }

    #[test]
    fn test_rec_set_bit_field_1_and_get_bit_field_1() {
        let (rec, layout, ptr) = get_new_rec();

        // Set a bit field
        rec.rec_set_bit_field_1(3, 1, 0x0F, 0);

        // Get the bit field
        let val = rec.rec_get_bit_field_1(1, 0x0F, 0);
        assert_eq!(val, 3);

        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_set_bit_field_2_and_get_bit_field_2() {
        let (rec, layout, ptr) = get_new_rec();

        // Set a bit field
        rec.rec_set_bit_field_2(300, 1, 0x0FFF, 0);

        // Get the bit field
        let val = rec.rec_get_bit_field_2(1, 0x0FFF, 0);
        assert_eq!(val, 300);

        free_rec(layout, ptr);
    }

    #[test]
    fn test_rec_get_field_start_offs() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // The first field starts at offset 0
        assert_eq!(rec.rec_get_field_start_offs(0), 0);

        // The second field starts after the first field
        assert_eq!(rec.rec_get_field_start_offs(1), 5); // After "Hello"
    }

    #[test]
    fn test_rec_get_nth_field_offs() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello".to_string())),
                FieldType::String(Some("World".to_string())),
            ],
        );
        let rec = owning_page.page.page_get_user_records()[0];

        // Get the offset and size of the first field
        let (offset, size) = rec.rec_get_nth_field_offs(0);
        assert_eq!(offset, 0);
        assert_eq!(size, 5); // "Hello" has 5 bytes

        // Get the offset and size of the second field
        let (offset, size) = rec.rec_get_nth_field_offs(1);
        assert_eq!(offset, 5); // After "Hello"
        assert_eq!(size, 5); // "World" has 5 bytes
    }

    #[test]
    fn test_rec_set_1byte_offs_flag_custom() {
        let (rec, layout, ptr) = get_new_rec();

        // Set the 1-byte offset flag to true
        rec.rec_set_1byte_offs_flag(true);
        assert!(rec.rec_get_1byte_offs_flag());

        // Set the 1-byte offset flag to false
        rec.rec_set_1byte_offs_flag(false);
        assert!(!rec.rec_get_1byte_offs_flag());

        free_rec(layout, ptr);
    }

    #[test]
    fn test_get_page() {
        let owning_page = create_records(
            1,
            vec![FieldType::String(Some("Test".to_string()))],
        );
        let page = owning_page.page;
        let rec = page.page_get_user_records()[0];

        // The page containing the record should be the same as the original page
        let rec_page = rec.get_page();

        // Compare page properties to verify they're the same page
        assert_eq!(rec_page.page_get_space_id(), page.page_get_space_id());
        assert_eq!(rec_page.page_get_fil_offset(), page.page_get_fil_offset());
    }

    #[test]
    fn test_rec_to_string() {
        {
            let owning_page = create_records(
                1,
                vec![
                    FieldType::String(Some("Hello World".to_string())),
                    FieldType::String(Some("Mario".to_string())),
                    FieldType::String(Some("Mushroom".to_string())),
                ],
            );
            let rec = owning_page.page.page_get_user_records()[0];

            assert_eq!(
                format!("{}", rec),
                "PHYSICAL RECORD: n_fields 3; 1-byte offsets; info bits 0; n owned 0; fields  0:  Hello World; len: 11  1:  Mario; len: 5  2:  Mushroom; len: 8 "
            );
        }

        {
            let owning_page = create_records(
                1,
                vec![
                    FieldType::String(Some("Hello World".to_string())),
                    FieldType::UnsignedInt(Some(19989)),
                    // FieldType::Double(Some(234.29)),
                ],
            );
            let rec = owning_page.page.page_get_user_records()[0];

            assert_eq!(
                format!("{}", rec),
                "PHYSICAL RECORD: n_fields 2; 1-byte offsets; info bits 0; n owned 0; fields  0:  Hello World; len: 11  1:  \u{15}N\0\0\0\0\0\0; len: 8 "
            );
        }
    }

    #[test]
    fn test_rec_set_1byte_offs_flag() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello World".to_string())),
                FieldType::String(Some("Mario".to_string())),
                FieldType::String(Some("Mushroom".to_string())),
            ],
        );

        assert_eq!(
            owning_page.page.page_get_user_records()[0]
                .rec_get_1byte_offs_flag(),
            true
        );
    }

    #[test]
    #[ignore]
    fn test_rec_set_2byte_offs_flag() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello World".to_string())),
                FieldType::String(Some("Mario".to_string())),
                FieldType::String(Some("Mushroom".to_string())),
                FieldType::String(Some("Donut".to_string())),
                FieldType::String(Some("Star Wars".to_string())),
                FieldType::String(Some(random_string_with_seed(127, 42))),
            ],
        );

        assert_eq!(
            owning_page.page.page_get_user_records()[0]
                .rec_get_1byte_offs_flag(),
            false
        );
    }

    #[test]
    fn test_rec_offs_size_with_1byte_flag() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello World".to_string())),
                FieldType::String(Some("Mario".to_string())),
                FieldType::String(Some("Mushroom".to_string())),
                FieldType::String(Some("Donut".to_string())),
                FieldType::String(Some("Star Wars".to_string())),
            ],
        );

        let rec = owning_page.page.page_get_user_records()[0];

        assert_eq!(rec.rec_get_1byte_offs_flag(), true);
        // REC_N_EXTRA_BYTES = 6
        // Content of the record: Sum all bytes: 38
        // Field metadata = 5 * 1
        // Total = 6 + 38 + 5 = 49
        assert_eq!(rec.rec_offs_size(), 49);
    }

    #[test]
    #[ignore]
    fn test_rec_offs_size_with_2byte_flag() {
        let owning_page = create_records(
            1,
            vec![
                FieldType::String(Some("Hello World".to_string())),
                FieldType::String(Some("Mario".to_string())),
                FieldType::String(Some("Mushroom".to_string())),
                FieldType::String(Some("Donut".to_string())),
                FieldType::String(Some("Star Wars".to_string())),
                FieldType::String(Some(random_string_with_seed(127, 42))),
            ],
        );

        let rec = owning_page.page.page_get_user_records()[0];

        assert_eq!(rec.rec_get_1byte_offs_flag(), false);
        // REC_N_EXTRA_BYTES = 6
        // Content of the record: Sum all bytes: 38 + 127 = 165
        // Field metadata = 6 * 2
        // Total = 6 + 165 + 12 = 183
        assert_eq!(rec.rec_offs_size(), 183);
    }
}
