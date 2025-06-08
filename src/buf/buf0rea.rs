use crate::buf::buf0types::BufBlock;
use crate::fil::fil0fil::Fil;
use crate::innodb::innodb0types::{DbErr, PageNo, SpaceId, UNIV_PAGE_SIZE};
use crate::os::os0file::{IORequest, IOResult};
use crate::trx::trx0sys::TRX_SYS_SPACE;
use crate::ut::ut0dbg::ut_a;
use crate::ut_panic;
use std::sync::{Arc, RwLock};
use tracing::{error, warn};

/// Manages read operations for the buffer pool.
///
/// BufRea (Buffer Read Ahead) is responsible for handling page read operations,
/// including synchronous and asynchronous reads, as well as read-ahead strategies
/// to optimize I/O performance.
pub struct BufRea {
    /// Reference to the file system interface for I/O operations
    fil: Arc<RwLock<Fil>>,
}

impl BufRea {
    /// Creates a new buffer read manager.
    ///
    /// # Parameters
    /// * `buf_pool` - A reference to the buffer pool
    /// * `fil` - A reference to the file system interface
    ///
    /// # Returns
    /// A new BufRea instance
    pub fn new(fil: Arc<RwLock<Fil>>) -> Self {
        BufRea { fil }
    }

    /// Reads a page from disk into the buffer pool.
    ///
    /// This method performs a synchronous read of the specified page
    /// into the buffer pool if it's not already present.
    ///
    /// # Parameters
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

        let fil = self.fil.read().unwrap();
        let table_space_version = fil.space_get_version(space_id);

        self.buf_read_page_low(
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
    /// # Parameters
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
    ) -> DbErr {
        ut_a!(
            io_request == IORequest::SyncRead
                || io_request == IORequest::AsyncRead
        );

        let (space_id, page_no) = {
            let buf_block = buf_block.read().unwrap();
            let page_id =
                buf_block.buf_block_get_page().borrow().get_page_id();

            (page_id.space_id, page_id.page_no)
        };

        if space_id == TRX_SYS_SPACE as u32 {
            error!("trying to read the double write buffer page: {}", page_no);
            return DbErr::DbFail(format!(
                "trying to read the double write buffer page: {}",
                page_no
            ));
        }

        let page = {
            let buf_block = buf_block.read().unwrap();
            buf_block.get_page()
        };

        let io_result = {
            let mut fil_guard = self.fil.write().unwrap();
            fil_guard.io(
                io_request.clone(),
                batch,
                space_id,
                page_no,
                0,
                page.to_page_frame(),
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
            Err(err) => return err,
        }

        DbErr::DbSuccess
    }
}
