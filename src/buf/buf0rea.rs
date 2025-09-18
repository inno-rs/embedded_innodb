use std::sync::{Arc, Mutex, RwLock};

use tracing::error;

use crate::{
    buf::buf0types::BufBlock,
    fil::fil0fil::Fil,
    innodb::innodb0types::{DbErr, PageNo, SpaceId, UNIV_PAGE_SIZE},
    os::os0file::{IORequest, IOResult},
    trx::trx0sys::TRX_SYS_SPACE,
    ut::ut0dbg::ut_a,
    ut_panic,
};

/// Manages read operations for the buffer pool.
///
/// BufRea (Buffer Read Ahead) is responsible for handling page read operations,
/// including synchronous and asynchronous reads, as well as read-ahead strategies
/// to optimize I/O performance.
pub struct BufRea {
    /// Reference to the file system interface for I/O operations
    fil: Arc<Mutex<Fil>>,
}

impl BufRea {
    /// Creates a new buffer read manager.
    ///
    /// # Arguments
    /// * `buf_pool` - A reference to the buffer pool
    /// * `fil` - A reference to the file system interface
    ///
    /// # Returns
    /// A new BufRea instance
    pub fn new(fil: Arc<Mutex<Fil>>) -> Self {
        BufRea { fil }
    }

    /// Reads a page from disk into the buffer pool.
    ///
    /// This method performs a synchronous read of the specified page
    /// into the buffer pool if it's not already present.
    ///
    /// # Arguments
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    ///
    /// # Returns
    /// * `true` - If the read operation was initiated successfully
    /// * `false` - If the read operation failed
    pub fn buf_read_page(
        &self,
        io_request: IORequest,
        space_id: SpaceId,
        page_no: PageNo,
        buf_block: Arc<RwLock<BufBlock>>,
    ) -> bool {
        // TODO(rahul) Check if the page is from double write buffer.

        let fil = self.fil.lock().unwrap();
        let table_space_version = fil.space_get_version(space_id);

        let _ret = self.buf_read_page_low(
            io_request,
            false,
            buf_block,
            table_space_version.unwrap_or(-1),
        );
        true
    }

    /// Low-level implementation of page read operations.
    ///
    /// This method handles the details of reading a page from disk,
    /// including validation checks and interaction with the buffer pool.
    ///
    /// # Arguments
    /// * `io_request` - The type of I/O operation (sync or async)
    /// * `batch` - Whether this is part of a batch read operation
    /// * `space_id` - The ID of the tablespace containing the page
    /// * `page_no` - The page number within the tablespace
    /// * `table_space_version` - The version of the tablespace
    ///
    /// # Returns
    /// * `DbErr::DbSuccess` - If the read operation was successful
    /// * `DbErr::DbFail` - If the read operation failed
    fn buf_read_page_low(
        &self,
        io_request: IORequest,
        batch: bool,
        buf_block: Arc<RwLock<BufBlock>>,
        table_space_version: i64,
    ) -> Result<(), DbErr> {
        ut_a!(
            io_request == IORequest::SyncRead
                || io_request == IORequest::AsyncRead
        );

        let (space_id, page_no) = {
            let buf_block = buf_block.read().unwrap();
            let page_id = buf_block.buf_block_get_page().get_page_id();

            (page_id.space_id, page_id.page_no)
        };

        if space_id == TRX_SYS_SPACE as u32 {
            error!("trying to read the double write buffer page: {}", page_no);
            return Err(DbErr::Fail(format!(
                "trying to read the double write buffer page: {}",
                page_no
            )));
        }

        let page = {
            let buf_block = buf_block.read().unwrap();
            buf_block.get_page()
        };

        let io_result = {
            let mut fil_guard = self.fil.lock().unwrap();
            fil_guard.io(
                io_request.clone(),
                batch,
                space_id,
                page_no,
                0,
                page.to_page_frame().0,
                UNIV_PAGE_SIZE,
            )
        };

        match io_result {
            Ok(result) => match result {
                IOResult::Read(read_result) => {}
                IOResult::Write(_) => {
                    ut_panic!("only write result expected here");
                }
            },
            Err(err) => return Err(err),
        }

        Ok(())
    }
}
