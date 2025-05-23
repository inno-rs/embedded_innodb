#[cfg(test)]
mod tests {
    use crate::buf::buf0types::BufPool;

    #[test]
    fn create_buf_pool() {
        let buf_pool = BufPool::new();
        let page = buf_pool.hash_get_page(0, 0);

        assert!(page.is_none());
    }
}
