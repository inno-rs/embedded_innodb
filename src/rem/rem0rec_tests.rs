#[cfg(test)]
mod tests {
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::rem::rem0rec::*;
    use std::alloc::{alloc, Layout};

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

    #[test]
    fn test_page_align() {
        let layout =
            Layout::from_size_align(UNIV_PAGE_SIZE, UNIV_PAGE_SIZE).unwrap();
        let p = unsafe { alloc(layout) };
        let rec_ptr = unsafe { p.add(897) };
        let rec = Record::new(rec_ptr);
        let aligned_ptr = rec.page_align();
        assert_eq!(aligned_ptr as usize % UNIV_PAGE_SIZE, 0);
    }

    #[test]
    fn test_rec_set_n_fields() {
        let (rec, _, _) = get_new_rec();

        for n_fields_val in (0..0xF).step_by(23) {
            println!("Setting n_fields to: {}", n_fields_val);
            println!("previous n_fields: {}", rec.rec_get_n_fields());

            rec.rec_set_n_fields(n_fields_val);
            let result = rec.rec_get_n_fields();

            assert_eq!(n_fields_val, result);
        }
    }

    #[test]
    fn test_rec_set_next_offs() {
        let (rec, layout, p) = get_new_rec();

        for next_offset_val in (0..0xFFFF).step_by(3453) {
            println!("previous next_offset: {}", rec.rec_get_next_offs());

            rec.rec_set_next_offs(next_offset_val);
            let result = rec.rec_get_next_offs();

            assert_eq!(next_offset_val, result);
        }
    }

    #[test]
    fn test_rec_set_info_bits() {
        let (rec, _, _) = get_new_rec();

        for info_bits_val in 0..0xF {
            println!("previous info_bits: {}", rec.rec_get_info_bits());

            rec.rec_set_info_bits(info_bits_val);
            let result = rec.rec_get_info_bits();
            assert_eq!(info_bits_val, result);
        }
    }

    #[test]
    fn test_rec_set_n_owned() {
        let (rec, _, _) = get_new_rec();

        for n_owned_val in 0..0xF {
            println!("previous n_owned: {}", rec.rec_get_n_owned());

            rec.rec_set_n_owned(n_owned_val);
            let result = rec.rec_get_n_owned();
            assert_eq!(n_owned_val, result);
        }
    }

    #[test]
    fn test_rec_set_heap_no() {
        let (rec, _, _) = get_new_rec();

        for heap_no_val in (0..8191).step_by(19) {
            println!("previous heap_no: {}", rec.rec_get_heap_no());

            rec.rec_set_heap_no(heap_no_val);
            let result = rec.rec_get_heap_no();
            assert_eq!(heap_no_val, result);
        }
    }
}
