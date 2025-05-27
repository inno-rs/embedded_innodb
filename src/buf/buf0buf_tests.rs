#[cfg(test)]
mod tests {
    use crate::buf::buf0buf::BufPool;
    use crate::fil::fil0fil::Fil;
    use std::sync::{Arc, RwLock};

    #[test]
    fn create_buf_pool() {
        let fil = Arc::new(RwLock::new(Fil::new(1)));
        let buf_pool = BufPool::new(fil);
        let page = buf_pool.hash_get_page(0, 0);

        assert!(page.is_none());
    }
}
