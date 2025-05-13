use crate::innodb::innodb0types::Lsn;

pub struct BufPage {
    /// Log Sequence Number of the youngest modification to the page. Zero, if the page is not modified.
    newest_modification: Lsn,
    /// Log Sequence Number of the oldest modification which has not yet been flushed to disk.
    /// Zero, if the page is not modified.
    oldest_modification: Lsn,
}

impl BufPage {
    pub fn new() -> Self {
        BufPage { newest_modification: 0, oldest_modification: 0 }
    }

    pub fn get_newest_modification(&self) -> Lsn {
        self.newest_modification
    }

    pub fn set_newest_modification(&mut self, lsn: Lsn) {
        self.newest_modification = lsn;
    }

    pub fn get_oldest_modification(&self) -> Lsn {
        self.oldest_modification
    }

    pub fn set_oldest_modification(&mut self, lsn: Lsn) {
        self.oldest_modification = lsn;
    }
}

#[derive(Clone)]
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
