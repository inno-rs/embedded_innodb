pub struct BufBlock {
    frame: Frame,
}
// The frame is a pointer to a memory location where the block's data is stored.
pub type Frame = *mut u8;

impl BufBlock {
    pub fn new(frame: Frame) -> Self {
        BufBlock { frame }
    }

    pub fn getFrame(&self) -> Frame {
        self.frame
    }
}
