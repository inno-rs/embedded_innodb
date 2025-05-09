#[cfg(test)]
mod tests {
    use super::super::page0page::Page;
    use crate::buf::buf0types::{BufBlock, Frame};
    use crate::dict::dict0types::Index;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::page::page0page::*;
    use std::alloc::{alloc_zeroed, Layout};

    fn get_raw_block() -> Frame {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        unsafe { alloc_zeroed(layout) }
    }

    #[test]
    fn test_page_create() {
        let index = Index::new();
        let buf_block = BufBlock::new(get_raw_block());
        let mtr = None;
        let page = Page::page_create(&index, &buf_block, mtr);

        let infimum_rec = page.page_get_infimum_rec();
        assert_eq!(infimum_rec.rec_get_data(0).unwrap(), "infimumr");

        let supremum_rec = page.page_get_supremum_rec();
        println!("{:?}", supremum_rec.rec_get_n_fields());
        assert_eq!(supremum_rec.rec_get_data(0).unwrap(), "supremumr");

        assert_eq!(page.page_header_get_field(PAGE_N_DIR_SLOTS), 2);
        assert_eq!(
            page.page_header_get_field(PAGE_HEAP_TOP),
            PAGE_SUPREMUM_END
        );
        assert_eq!(
            page.page_header_get_field(PAGE_N_HEAP),
            PAGE_HEAP_NO_USER_LOW
        );
        assert_eq!(page.page_header_get_field(PAGE_FREE), u16::MAX as usize);
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
        // assert_eq!(
        //     page.page_dir_slot_get_rec(0).get_ptr() as usize,
        //     infimum_rec.get_ptr() as usize
        // );

        // assert_eq!(
        //     page.page_dir_slot_get_rec(1).get_ptr() as usize,
        //     supremum_rec.get_ptr() as usize
        // );

        // Check next offsets of the infimum and supremum records
        assert_eq!(infimum_rec.rec_get_next_offs(), PAGE_SUPREMUM);
        assert_eq!(page.page_get_supremum_rec().rec_get_next_offs(), 0);
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
    }

    #[test]
    fn test_page_set_header_field() {
        let ptr = get_raw_block();
        let page = Page::new(ptr);

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
    }
}
