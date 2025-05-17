use crate::innodb::innodb0types::{Lsn, PageId};
use intrusive_collections::{intrusive_adapter, LinkedListLink};
use std::collections::{HashMap, LinkedList};

pub struct BufPage {
    /// Log Sequence Number of the youngest modification to the page. Zero, if the page is not modified.
    newest_modification: Lsn,
    /// Log Sequence Number of the oldest modification which has not yet been flushed to disk.
    /// Zero, if the page is not modified.
    oldest_modification: Lsn,
    /// List of free pages.
    free_list: LinkedListLink,
    /// List of nodes.
    flush_list: LinkedListLink,
    /// Node of the LRU list.
    lru_list: LinkedListLink,
}

intrusive_adapter!(FlushListAdapter = Box<BufPage>: BufPage { flush_list: LinkedListLink });
intrusive_adapter!(FreeListAdapter = Box<BufPage>: BufPage { free_list: LinkedListLink });
intrusive_adapter!(LruAdapter = Box<BufPage>: BufPage { lru_list: LinkedListLink });

impl BufPage {
    pub fn new() -> Self {
        BufPage {
            newest_modification: 0,
            oldest_modification: 0,
            free_list: Default::default(),
            flush_list: Default::default(),
            lru_list: Default::default(),
        }
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

/// The buffer control block structure.
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

pub struct BufPool {
    page_hash: HashMap<PageId, Box<BufBlock>>,
    flush_list: LinkedList<FlushListAdapter>,
    free_list: LinkedList<FreeListAdapter>,
    lru_list: LinkedList<LruAdapter>,
}

impl BufPool {
    pub fn new() -> Self {
        BufPool {
            page_hash: Default::default(),
            free_list: Default::default(),
            lru_list: Default::default(),
            flush_list: Default::default(),
        }
    }

    pub fn hash_get_page(
        &mut self,
        page_id: PageId,
    ) -> Option<&Box<BufBlock>> {
        self.page_hash.get(&page_id)
    }
}
