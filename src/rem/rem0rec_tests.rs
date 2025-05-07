#[cfg(test)]
mod tests {
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::rem::rem0rec::*;
    use std::alloc::{alloc, Layout};

    #[test]
    fn test_rem0rec() {
        // Test cases for rem0rec functions
        // Add your test cases here
        assert_eq!(1, 1); // Placeholder assertion
    }

    fn get_new_rec() -> Record {
        let layout = Layout::from_size_align(6, UNIV_PAGE_SIZE).unwrap();
        let ptr = unsafe { alloc(layout) };
        Record::new(ptr)
    }
    #[test]
    fn test_get_n_fields() {
        
    }
}
