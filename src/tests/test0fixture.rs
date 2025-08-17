use std::sync::{Arc, Mutex};

use tokio::runtime::Runtime;

use crate::{
    buf::{
        buf0buf::BufPool,
        buf0dblwr::{
            DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
        },
        buf0flu::BufFlush,
        buf0lru::BufLRU,
        buf0rea::BufRea,
    },
    dict::dict0dict::Dict,
    fil::fil0fil::Fil,
    fsp::fsp0fsp::Fsp,
    innodb::innodb0types::UNIV_PAGE_SIZE,
    log::log0log::Log,
    os::{os0io::OsIo, os0runtime::create_runtime},
};

/// Common fixture for tests.
pub struct CommonFixture {
    pub runtime: Arc<Runtime>,
    pub os_io: Arc<Mutex<OsIo>>,
    pub fil: Arc<Mutex<Fil>>,
    pub buf_rea: Arc<Mutex<BufRea>>,
    pub buf_lru: Arc<Mutex<BufLRU>>,
    pub buf_pool: Arc<BufPool>,
    pub fsp: Arc<Mutex<Fsp>>,
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
        let block = Box::new(WriteBuffer([0u8; DOUBLE_WRITE_BUFFER_SIZE]));
        let dblwr_buf = Arc::new(Mutex::new(DoubleWriteBuffer::new(
            block,
            fil.clone(),
            UNIV_PAGE_SIZE,
        )));
        let redo_log = Arc::new(Mutex::new(Log::new(
            UNIV_PAGE_SIZE,
            fil.clone(),
            Default::default(),
        )));
        let flusher = Arc::new(Mutex::new(BufFlush::new(
            dblwr_buf.clone(),
            fil.clone(),
            redo_log.clone(),
        )));
        let buf_rea = Arc::new(Mutex::new(BufRea::new(fil.clone())));
        let buf_lru = Arc::new(Mutex::new(BufLRU::new()));
        let buf_pool = Arc::new(BufPool::new(
            fil.clone(),
            buf_lru.clone(),
            buf_rea.clone(),
            flusher.clone(),
        ));

        let fsp =
            Arc::new(Mutex::new(Fsp::new(buf_pool.clone(), fil.clone())));

        let dict = Arc::new(Dict::new(fsp.clone()));

        Self { runtime, os_io, fil, buf_rea, buf_lru, buf_pool, fsp, dict }
    }
}

impl Default for CommonFixture {
    fn default() -> Self {
        CommonFixture::new()
    }
}
