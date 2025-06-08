pub struct Config {
    /// Path to the data directory.
    pub data_home: String,
    /// Size of the log buffer in pages.
    pub log_buffer_size: usize,
    /// Size of the buffer pool in pages.
    pub buf_pool_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_home: "/tmp/".to_string(),
            log_buffer_size: 128, // Default size in pages
            buf_pool_size: 1024,  // Default size in pages
        }
    }
}
