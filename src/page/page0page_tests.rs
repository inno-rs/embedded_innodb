#[cfg(test)]
mod tests {
    use super::super::page0page::Page;
    use crate::buf::buf0types::{BufBlock, Frame};
    use crate::dict::dict0types::Index;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::page::page0page::*;
    use crate::rem::rem0rec::{Record, RecordData};
    use std::alloc::{alloc_zeroed, Layout};

    fn get_raw_block() -> Frame {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { alloc_zeroed(layout) }
    }

    fn free_raw_block(frame: Frame) {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { std::alloc::dealloc(frame, layout) }
    }

    #[test]
    fn test_page_create() {
        let index = Index::new();
        let frame = get_raw_block();
        let buf_block = BufBlock::new(frame);
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        let infimum_rec = page.page_get_infimum_rec();
        let extract_data = |rec: Record| -> String {
            return match rec.rec_get_data(0).unwrap() {
                RecordData::Data(d) => d,
                RecordData::SqlNullData() => {
                    panic!("SQL NULL DATA");
                }
            };
        };

        assert_eq!(extract_data(infimum_rec), "infimumr");

        let supremum_rec = page.page_get_supremum_rec();
        println!("{:?}", supremum_rec.rec_get_n_fields());
        assert_eq!(extract_data(supremum_rec), "supremumr");

        assert_eq!(page.page_header_get_field(PAGE_N_DIR_SLOTS), 2);
        assert_eq!(
            page.page_header_get_field(PAGE_HEAP_TOP),
            PAGE_SUPREMUM_END
        );
        assert_eq!(
            page.page_header_get_field(PAGE_N_HEAP),
            PAGE_HEAP_NO_USER_LOW
        );
        assert_eq!(page.page_header_get_field(PAGE_FREE), 0);
        assert_eq!(page.page_header_get_field(PAGE_GARBAGE), 0);
        assert_eq!(
            page.page_header_get_field(PAGE_LAST_INSERT),
            u16::MAX as usize
        );
        assert_eq!(
            page.page_header_get_field(PAGE_DIRECTION),
            PAGE_NO_DIRECTION
        );
        assert_eq!(page.page_header_get_field(PAGE_N_DIRECTION), 0);
        assert_eq!(page.page_header_get_field(PAGE_N_RECS), 0);
        assert_eq!(page.get_max_trx_id(), 0);

        // Check the slots in the page header
        assert_eq!(
            page.page_dir_slot_get_rec(0).get_ptr() as usize,
            infimum_rec.get_ptr() as usize
        );

        assert_eq!(
            page.page_dir_slot_get_rec(1).get_ptr() as usize,
            supremum_rec.get_ptr() as usize
        );

        // Check next offsets of the infimum and supremum records
        assert_eq!(infimum_rec.rec_get_next_offs(), PAGE_SUPREMUM);
        assert_eq!(page.page_get_supremum_rec().rec_get_next_offs(), 0);

        assert!(page.page_header_get_page_free().is_none());

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_get_n_recs() {
        let index = Index::new();
        let frame = get_raw_block();
        let buf_block = BufBlock::new(frame);
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        assert_eq!(page.page_n_get_recs(), 0);

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_get_next_record() {
        let index = Index::new();
        let frame = get_raw_block();
        let buf_block = BufBlock::new(frame);
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();
        let supremum_next = supremum_rec.rec_get_next();

        assert!(page.page_rec_is_infimum(infimum_rec));
        assert!(page.page_rec_is_supremum(supremum_rec));

        assert_eq!(supremum_next, None);

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_rec_is_user_record() {
        let index = Index::new();
        let frame = get_raw_block();
        let buf_block = BufBlock::new(frame);
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();

        assert!(!page.page_rec_is_user_rec(infimum_rec));
        assert!(!page.page_rec_is_user_rec(supremum_rec));

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_dir_find_owner_slot() {
        let index = Index::new();
        let frame = get_raw_block();
        let buf_block = BufBlock::new(frame);
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        let infimum_rec = page.page_get_infimum_rec();
        let supremum_rec = infimum_rec.rec_get_next().unwrap();

        assert_eq!(page.page_dir_find_owner_slot(infimum_rec), 0);
        assert_eq!(page.page_dir_find_owner_slot(supremum_rec), 1);

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_set_max_trx_id() {
        let frame = get_raw_block();
        let page = Page::new(frame);
        let max_trx_id: crate::utils::TxnId = 2352325;
        let mtr = None; // Assuming Mtr is not used in this test

        page.set_max_trx_id(max_trx_id, mtr);

        let retrieved_max_trx_id = page.get_max_trx_id();
        assert_eq!(retrieved_max_trx_id, max_trx_id);

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_set_header_field() {
        let frame = get_raw_block();
        let page = Page::new(frame);

        {
            let field_offset: usize = PAGE_N_DIR_SLOTS;
            let value = 42;

            page.page_header_set_field(field_offset, value as usize);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_HEAP_TOP;
            let value = 44;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_HEAP;
            let value = 46;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_FREE;
            let value = 47;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_GARBAGE;
            let value = 48;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LAST_INSERT;
            let value = 54;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_DIRECTION;
            let value = 56;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_DIRECTION;
            let value = 58;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_RECS;
            let value = 60;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_MAX_TRX_ID;
            let value = 62;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LEVEL;
            let value = 64;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_INDEX_ID;
            let value = 66;

            page.page_header_set_field(field_offset, value);

            let retrieved_value = page.page_header_get_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        // Free the allocated memory
        free_raw_block(frame);
    }

    #[test]
    fn test_page_dir_calc_reserved_space() {
        let val = Page::page_dir_calc_reserved_space(1);
        assert_eq!(val, 1);

        let val = Page::page_dir_calc_reserved_space(100);
        assert_eq!(val, 50);
    }

    #[test]
    fn test_page_get_free_space_of_empty() {
        assert_eq!(16191, Page::page_get_free_space_of_empty());
    }

    #[test]
    fn test_page_get_max_insert_size() {
        // Empty page
        {
            let index = Index::new();
            let frame = get_raw_block();
            let buf_block = BufBlock::new(frame);
            let mtr = None;
            let page = Page::page_create(&index, &buf_block, mtr);

            let size = page.page_get_max_insert_size(1);
            assert_eq!(size, 16190);

            // Free the allocated memory
            free_raw_block(frame);
        }
    }
}
