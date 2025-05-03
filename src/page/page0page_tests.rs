#[cfg(test)]
mod tests {
    use super::super::page0page::Page;
    use crate::page::page0page::*;
    use std::alloc::{alloc, Layout};

    fn get_raw_block() -> *mut u8 {
        let layout = Layout::from_size_align(1024, 1).unwrap();
        unsafe { alloc(layout) }
    }

    #[test]
    fn test_page_set_max_trx_id() {
        let ptr = get_raw_block();
        let page = Page::new(ptr);
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

            page.page_set_header_field(field_offset, value as usize);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_HEAP_TOP;
            let value = 44;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_HEAP;
            let value = 46;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_FREE;
            let value = 47;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_GARBAGE;
            let value = 48;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LAST_INSERT;
            let value = 54;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_DIRECTION;
            let value = 56;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_DIRECTION;
            let value = 58;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_N_RECS;
            let value = 60;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_MAX_TRX_ID;
            let value = 62;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_LEVEL;
            let value = 64;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }

        {
            let field_offset: usize = PAGE_INDEX_ID;
            let value = 66;

            page.page_set_header_field(field_offset, value);

            let retrieved_value = page.page_get_header_field(field_offset);
            assert_eq!(retrieved_value, value);
        }
    }
}
