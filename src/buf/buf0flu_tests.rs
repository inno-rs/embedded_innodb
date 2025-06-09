#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex, RwLock};

    use rstest::{fixture, rstest};

    use crate::buf::buf0buf::BufPool;
    use crate::buf::buf0dblwr::{
        DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
    };
    use crate::buf::buf0flu::BufFlush;
    use crate::buf::buf0lru::BufLRU;
    use crate::buf::buf0rea::BufRea;
    use crate::fil::fil0fil::Fil;
    use crate::fsp::fsp0fsp::Fsp;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::log::log0log::Log;
    use crate::os::os0io::OsIo;
    use crate::os::os0runtime::create_runtime;

    struct BufFluFixture {
        buf_flu: BufFlush,
    }

    impl BufFluFixture {
        pub fn new() -> Self {
            tracing::info!(
                "Setting up BufFlush fixture, total double write buffer size: {}",
                DOUBLE_WRITE_BUFFER_SIZE
            );

            let runtime = create_runtime(1);
            let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
            let fil =
                Arc::new(Mutex::new(Fil::new(100, os_io.clone(), runtime)));
            let rea = Arc::new(RefCell::new(BufRea::new(fil.clone())));
            let buf_lru = Arc::new(RefCell::new(BufLRU::new()));
            let buf_pool =
                Arc::new(RwLock::new(BufPool::new(fil.clone(), buf_lru, rea)));
            let fsp = Arc::new(RwLock::new(Fsp::new(buf_pool.clone())));
            let block = Box::new(WriteBuffer([0u8; DOUBLE_WRITE_BUFFER_SIZE]));
            let dblwr_buf = Arc::new(Mutex::new(DoubleWriteBuffer::new(
                fsp,
                block,
                fil.clone(),
                UNIV_PAGE_SIZE,
            )));

            let redo_log = Arc::new(Mutex::new(Log::new(UNIV_PAGE_SIZE)));
            Self { buf_flu: BufFlush::new(dblwr_buf, fil, redo_log) }
        }
    }

    #[fixture]
    fn buf_flu_fixture() -> BufFluFixture {
        BufFluFixture::new()
    }

    #[rstest]
    fn test_buf_flush_list_count(buf_flu_fixture: BufFluFixture) {
        let buf_flu = buf_flu_fixture.buf_flu;
        assert_eq!(buf_flu.get_flush_list_size(), 0);
    }
}
