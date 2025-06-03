use crate::buf::buf0dblwr::DoubleWriteBuffer;
use crate::buf::buf0types::BufPageState::BufBlockFilePage;
use crate::buf::buf0types::{BufBlock, FlushListAdapter};
use crate::ut::ut0dbg::ut_a;
use intrusive_collections::LinkedList;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

pub struct BufFlush {
    dblwr: Arc<Mutex<DoubleWriteBuffer>>,
    /// List of buffer blocks to flush.
    flush_list: Arc<RefCell<LinkedList<FlushListAdapter>>>,
}

impl BufFlush {
    pub fn new(dblwr: Arc<Mutex<DoubleWriteBuffer>>) -> Self {
        Self { dblwr, flush_list: Default::default() }
    }

    pub fn post_to_double_write_buf(&mut self, buf_block: &mut BufBlock) {
        let buf_page = buf_block.buf_block_get_page();
        let buf_page_ref = buf_page.borrow_mut();

        ut_a!(buf_page_ref.get_state() == BufBlockFilePage);

        let raw_page = buf_block.get_page();
    }
}
