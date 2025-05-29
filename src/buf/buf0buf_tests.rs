#[cfg(test)]
mod tests {
    use crate::buf::buf0buf::BufPool;
    use crate::fil::fil0fil::Fil;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::mtr::mtr0mtr::Mtr;
    use crate::mtr::mtr0mtr::MtrState::MTR_ACTIVE;
    use std::sync::{Arc, RwLock};

    #[test]
    fn create_buf_pool() {
        let fil = Arc::new(RwLock::new(Fil::new(1)));
        let buf_pool = BufPool::new(fil);
        let page = buf_pool.hash_get_page(0, 0);

        assert!(page.is_none());
        assert!(!buf_pool.peek(0, 1));
    }

    #[test]
    fn test_free_list_returns_none() {
        let fil = Arc::new(RwLock::new(Fil::new(1)));
        let mut buf_pool = BufPool::new(fil);

        let r = buf_pool.get_free_block();
        assert!(r.is_none());
    }

    #[test]
    fn test_free_list_returns_some() {
        let fil = Arc::new(RwLock::new(Fil::new(1)));
        let mut buf_pool = BufPool::new(fil);

        let result = buf_pool.chunk_init(10 * UNIV_PAGE_SIZE);

        assert_eq!(buf_pool.get_free_list_size(), 10);
        assert_eq!(result.size, 10);
    }

    // #[ignore]
    #[test]
    fn test_create_space() {
        let fil = Arc::new(RwLock::new(Fil::new(1)));
        let mut buf_pool = BufPool::new(fil);
        let mut mtr = Mtr::new();
        mtr.set_mtr_state(MTR_ACTIVE);

        // Try creating when there are no pages in the pool.
        let result = buf_pool.create(0, 0, &mut mtr);

        assert!(result.is_none());

        // Add some pages, and then retry creating.
        let _result = buf_pool.chunk_init(10 * UNIV_PAGE_SIZE);
        assert_eq!(buf_pool.get_free_list_size(), 10);

        let resp = buf_pool.create(0, 0, &mut mtr);
        assert!(resp.is_some());

        // The total number of items in the free list should decrease by 1.
        assert_eq!(buf_pool.get_free_list_size(), 9);

        for i in 0..9 {
            let resp = buf_pool.create(0, 0, &mut mtr);
            assert!(resp.is_some());

            assert_eq!(buf_pool.get_free_list_size(), 9 - i - 1);
        }
    }
}
