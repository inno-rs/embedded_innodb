use crate::fsp::fspOfsp::Fsp;
use crate::innodb::innodb0types::UNIV_PAGE_SIZE;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use std::fmt::{Debug, Pointer};
use std::ops::DerefMut;
use std::sync::{Arc, RwLock};

pub const DOUBLE_WRITE_BUFFER_SIZE: usize = 1 + 2 * 128 * UNIV_PAGE_SIZE;
pub struct Block([u8; DOUBLE_WRITE_BUFFER_SIZE]);
pub struct PageFrame([u8; UNIV_PAGE_SIZE]);
pub struct DoubleWriteBuffer {
    fsp: Arc<RwLock<Fsp>>,
    block: Box<Block>,
    first_free: usize,
}

impl DoubleWriteBuffer {
    pub fn new(fsp: Arc<RwLock<Fsp>>, block: Box<Block>) -> Self {
        let ptr = block.as_ref() as *const Block as *const u8;
        // Ensure the block is properly aligned
        ut_a!(
            ptr as usize % UNIV_PAGE_SIZE == 0,
            "DoubleWriteBuffer block is not properly aligned"
        );
        Self { fsp, block, first_free: 0 }
    }

    pub fn append(&mut self, page_frame: &PageFrame) {
        let chunk = self
            .block
            .deref_mut()
            .0
            .chunks_mut(UNIV_PAGE_SIZE)
            .skip(self.first_free - 1)
            .next();

        ut_ad!(chunk.is_some(), "No space left in DoubleWriteBuffer");
        let chunk = chunk.unwrap();

        chunk.copy_from_slice(&page_frame.0);
        self.first_free += 1;
    }
}
