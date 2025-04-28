/* PAGE HEADER
Index page header starts at the first offset left free by the fil-module.
 */
use crate::buf0buf::BufBlock;
use crate::fil0types::FSEG_PAGE_DATA;
use crate::mtr0mtr::mtr;
use crate::utils::TxnId;

struct PageConstants {}
impl PageConstants {
    /** Index page header starts at this offset */
    pub const PAGE_HEADER: usize = FSEG_PAGE_DATA;

    pub const PAGE_SIZE: usize = 16 * 1024;

    /** Highest id of the transaction that may have modified a record on the page; a i64; defined only in the secondary indexes; NOTE: this may be modified only when the thread has an x-latch to the page. */
    pub const PAGE_MAX_TRX_ID: usize = 18;
}

struct Page {
    data: [u8; PageConstants::PAGE_SIZE],
}

impl Page {
    pub fn new() -> Self {
        Page {
            data: [0; PageConstants::PAGE_SIZE],
        }
    }

    pub fn set_max_trx_id(buf_block: &BufBlock, max_trx_id: TxnId, mtr: Option<mtr>) {
        let mut page = buf_block.getFrame();
        /* It is not necessary to write this change to the redo log as
        during the database recovery we assume that the max transaction id
        of every pae is the maximum transaction id assigned before the crash.
        */
        match mtr {
            None => unsafe {
                page.as_mut_ptr()
                    .add(PageConstants::PAGE_HEADER + PageConstants::PAGE_MAX_TRX_ID)
                    .write(max_trx_id as u8);
            },
            Some(_) => {
                log::warn!("don't know how to handle mtr");
            }
        }
    }
}
