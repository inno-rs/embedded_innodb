#[cfg(test)]
mod tests {
    use crate::buf::buf0dblwr::{
        Block, DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer,
    };
    use crate::buf::buf0flu::BufFlush;
    use crate::fil::fil0fil::Fil;
    use crate::fsp::fsp0fsp::Fsp;
    use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
    use crate::log::log0log::Log;
    use crate::os::os0io::OsIo;
    use crate::ut::ut0dbg::init_tracing;
    use rstest::{fixture, rstest};
    use std::sync::{Arc, Mutex, RwLock};

    struct BufFluFixture {
        buf_flu: BufFlush,
    }

    impl BufFluFixture {
        pub fn new() -> Self {
            init_tracing();

            tracing::info!(
                "Setting up BufFlush fixture, total double write buffer size: {}",
                DOUBLE_WRITE_BUFFER_SIZE
            );
            let fsp = Arc::new(RwLock::new(Fsp::new()));
            let block = Box::new(Block([0u8; DOUBLE_WRITE_BUFFER_SIZE]));
            let dblwr_buf = Arc::new(Mutex::new(DoubleWriteBuffer::new(
                fsp,
                block,
                UNIV_PAGE_SIZE,
            )));

            let os_io = Arc::new(Mutex::new(OsIo::new()));
            let fil = Arc::new(Mutex::new(Fil::new(100, os_io)));
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
