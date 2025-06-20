use std::sync::{Arc, Mutex};

use derive_builder::Builder;

use crate::{
    fil::{fil0fil::Fil, fil0types::FilType::FilLog},
    innodb::innodb0types::SpaceId,
    log::log0log::LOG_SPACE_FIRST_ID,
    ut::ut0dbg::ut_ad,
};

pub struct Config {
    /// Path to the data directory.
    pub data_home: String,
    /// Size of the log buffer in pages.
    pub log_buffer_size: usize,
    /// Size of the buffer pool in pages.
    pub buf_pool_size: usize,
    /// Path to the redo log directory.
    pub log_dir: String,
    /// Number of log files.
    pub n_log_files: usize,
    /// Size of the log file in pages.
    pub log_file_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_home: "/tmp/".to_string(),
            log_buffer_size: 128, // Default size in pages
            buf_pool_size: 1024,  // Default size in pages
            log_dir: "/tmp/".to_string(),
            n_log_files: 2,
            log_file_size: 200, // Default size in pages
        }
    }
}

#[derive(Clone, Builder)]
pub struct Innodb {
    /// Immutable reference to config.
    config: Arc<Config>,
    /// Reference to fil.
    fil: Arc<Mutex<Fil>>,
}

impl Innodb {
    pub fn start(&self) {
        {
            // Step # 10
            // Configure the redo log.
            ut_ad!(self.config.log_dir.is_empty() == false);
            ut_ad!(self.config.log_file_size > 0);

            let log_file_name = "ib_logfile0".to_string();

            let mut fil = self.fil.lock().unwrap();
            let ret = fil.space_create(
                &log_file_name,
                LOG_SPACE_FIRST_ID as SpaceId,
                0,
                FilLog,
            );

            ut_ad!(ret);

            // Create filnode.
            let ret = fil.node_create(
                log_file_name.as_str(),
                self.config.log_file_size,
                LOG_SPACE_FIRST_ID as SpaceId,
                false,
            );

            ut_ad!(ret);
        }
    }
}
