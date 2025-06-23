#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rstest::{fixture, rstest};
    use tokio::runtime::Runtime;

    use crate::{
        buf::buf0dblwr::{
            DOUBLE_WRITE_BUFFER_SIZE, DoubleWriteBuffer, WriteBuffer,
        },
        fil::{fil0fil::Fil, fil0types::FilType},
        innodb::innodb0types::{PageId, UNIV_PAGE_SIZE},
        os::{os0file::IORequest, os0io::OsIo, os0runtime::create_runtime},
    };

    struct DblWrBufferFixture {
        fil: Arc<Mutex<Fil>>,
        dblwr_buf: DoubleWriteBuffer,
    }

    #[fixture]
    fn runtime() -> Arc<Runtime> {
        create_runtime(1)
    }

    #[fixture]
    fn os_io(runtime: Arc<Runtime>) -> Arc<Mutex<OsIo>> {
        Arc::new(Mutex::new(OsIo::new(runtime)))
    }

    #[fixture]
    fn fil(os_io: Arc<Mutex<OsIo>>, runtime: Arc<Runtime>) -> Arc<Mutex<Fil>> {
        Arc::new(Mutex::new(Fil::new(10, os_io, runtime)))
    }

    #[fixture]
    fn dblwr_buf_fix(fil: Arc<Mutex<Fil>>) -> DblWrBufferFixture {
        let block = Box::new(WriteBuffer([0u8; DOUBLE_WRITE_BUFFER_SIZE]));
        DblWrBufferFixture {
            fil: fil.clone(),
            dblwr_buf: DoubleWriteBuffer::new(
                block,
                fil,
                DOUBLE_WRITE_BUFFER_SIZE / UNIV_PAGE_SIZE,
            ),
        }
    }

    #[rstest]
    #[should_panic]
    fn test_double_write_buffer_empty_flush(
        mut dblwr_buf_fix: DblWrBufferFixture,
    ) {
        dblwr_buf_fix.dblwr_buf.flush().unwrap()
    }

    #[rstest]
    fn test_new_double_write_buffer_is_empty(
        dblwr_buf_fix: DblWrBufferFixture,
    ) {
        assert!(dblwr_buf_fix.dblwr_buf.is_empty());
    }

    #[rstest]
    fn test_double_write_buffer_flush(dblwr_buf_fix: DblWrBufferFixture) {
        let mut dblwr_buf = dblwr_buf_fix.dblwr_buf;
        let fil = dblwr_buf_fix.fil;
        let mut data = b"Hello, world!".to_vec();
        data.resize(UNIV_PAGE_SIZE, 0);

        let result = dblwr_buf.append_low(
            PageId { space_id: 0, page_no: 0 },
            data.as_mut_slice(),
        );

        assert!(result.is_ok());

        // create the SystemTableSpace file.
        let system_table_space = fil_space_create(&fil);
        assert!(system_table_space);

        let flush_result = dblwr_buf.flush();
        assert!(flush_result.is_ok());

        // retrieve the data from file again.
        let system_table_space = {
            let mut resp_data = Box::new([0u8; UNIV_PAGE_SIZE]);
            let space_id = 0;
            let mut fil = fil.lock().unwrap();
            let result = fil.io(
                IORequest::SyncRead,
                false,
                0,
                0,
                0,
                resp_data.as_mut(),
                UNIV_PAGE_SIZE,
            );

            assert!(result.is_ok());
            assert_eq!(resp_data.as_slice(), data.as_slice());
        };

        assert!(dblwr_buf.is_empty());
    }

    #[rstest]
    fn test_double_write_buffer_multiple_flush(
        dblwr_buf_fix: DblWrBufferFixture,
    ) {
        let mut dblwr_buf = dblwr_buf_fix.dblwr_buf;
        let fil = dblwr_buf_fix.fil;
        let mut pages: Vec<Vec<u8>> = Default::default();

        for i in 0..10 {
            let mut data = b"Hello, world!".to_vec();
            data.resize(UNIV_PAGE_SIZE, 0);

            let result = dblwr_buf.append_low(
                PageId { space_id: 0, page_no: i },
                data.as_mut_slice(),
            );

            assert!(result.is_ok());
            pages.push(data);
        }

        let system_table_space = fil_space_create(&fil);
        assert!(system_table_space);

        let flush_result = dblwr_buf.flush();
        assert!(flush_result.is_ok());
        assert!(dblwr_buf.is_empty());

        for i in 0..10 {
            // retrieve the data from file again.
            let system_table_space = {
                let mut resp_data = Box::new([0u8; UNIV_PAGE_SIZE]);
                let space_id = 0;
                let mut fil = fil.lock().unwrap();
                let result = fil.io(
                    IORequest::SyncRead,
                    false,
                    0,
                    i,
                    0,
                    resp_data.as_mut(),
                    UNIV_PAGE_SIZE,
                );

                assert!(result.is_ok());
                assert_eq!(
                    resp_data.as_slice(),
                    (&pages[i as usize]).as_slice()
                );
            };
        }
    }

    fn fil_space_create(fil: &Arc<Mutex<Fil>>) -> bool {
        // create the SystemTableSpace file.
        let system_table_space = {
            let space_id = 0;
            let mut fil = fil.lock().unwrap();
            if fil.space_create(
                &"test".to_string(),
                space_id,
                0,
                FilType::FilTablespace,
            ) == false
            {
                false;
            }

            if fil.node_create("test", 10, space_id, false) == false {
                false;
            }

            fil.space_get_by_id(space_id).is_some()
        };
        system_table_space
    }
}
