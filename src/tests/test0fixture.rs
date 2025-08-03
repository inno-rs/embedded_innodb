use std::sync::{Arc, Mutex};

use tokio::runtime::Runtime;

use crate::{
    buf::{buf0buf::BufPool, buf0lru::BufLRU, buf0rea::BufRea},
    dict::dict0dict::Dict,
    fil::fil0fil::Fil,
    fsp::fsp0fsp::Fsp,
    os::{os0io::OsIo, os0runtime::create_runtime},
};

/// Common fixture for tests.
pub struct CommonFixture {
    pub runtime: Arc<Runtime>,
    pub os_io: Arc<Mutex<OsIo>>,
    pub fil: Arc<Mutex<Fil>>,
    pub buf_rea: Arc<Mutex<BufRea>>,
    pub buf_lru: Arc<Mutex<BufLRU>>,
    pub buf_pool: Arc<Mutex<BufPool>>,
    pub dict: Arc<Dict>,
}

impl CommonFixture {
    pub fn new() -> Self {
        let runtime = create_runtime(1);
        let os_io =
            Arc::new(std::sync::Mutex::new(OsIo::new(runtime.clone())));
        let fil = Arc::new(std::sync::Mutex::new(Fil::new(
            1,
            os_io.clone(),
            runtime.clone(),
        )));
        let buf_rea =
            Arc::new(std::sync::Mutex::new(BufRea::new(fil.clone())));
        let buf_lru = Arc::new(std::sync::Mutex::new(BufLRU::new()));
        let buf_pool = Arc::new(std::sync::Mutex::new(BufPool::new(
            fil.clone(),
            buf_lru.clone(),
            buf_rea.clone(),
        )));

        let fsp =
            Arc::new(Mutex::new(Fsp::new(buf_pool.clone(), fil.clone())));

        let dict = Arc::new(Dict::new(fsp.clone()));

        Self { runtime, os_io, fil, buf_rea, buf_lru, buf_pool, dict }
    }
}

impl Default for CommonFixture {
    fn default() -> Self {
        CommonFixture::new()
    }
}
