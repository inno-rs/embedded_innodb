use crate::fil::fil0types::SpaceId;
use crate::innodb::innodb0types::{Lsn, PageId, PageNo, UNIV_PAGE_SIZE};
use crate::mtr::mtr0mtr::{Mtr, MtrState};
use crate::page::page0page::Page;
use crate::sync::sync0rw::ReadWriteLatch;
use crate::ut::ut0dbg::ut_ad;
use intrusive_collections::{intrusive_adapter, LinkedList, LinkedListLink};
use std::collections::HashMap;
use std::sync::RwLock;

// The frame is a pointer to a memory location where the block's data is stored.
pub type Frame = *mut u8;

pub struct BufPage {
    /// This tells us the last time when this page was put at the head of the LRU list.
    freed_page_lock: usize,
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
            freed_page_lock: 0,
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

impl BufBlock {
    pub fn new(frame: Frame) -> Self {
        BufBlock { frame }
    }

    pub fn getFrame(&self) -> Frame {
        self.frame
    }
}

pub struct BufferBlock {
    buf_frame: Frame,
    lock: RwLock<()>,
}

impl BufferBlock {
    pub fn new(ptr: *mut u8) -> BufferBlock {
        let block = BufferBlock { buf_frame: ptr, lock: Default::default() };
        block
    }

    pub fn get_page(&self) -> Page {
        Page::new(self.buf_frame)
    }
}

pub struct BufPool {
    page_hash: HashMap<PageId, Box<BufferBlock>>,
    flush_list: LinkedList<FlushListAdapter>,
    free_list: LinkedList<FreeListAdapter>,
    lru_list: LinkedList<LruAdapter>,
}

pub struct Request<'a> {
    pub rw_latch: ReadWriteLatch,
    pub page_id: PageId,
    pub mtr: &'a mut Mtr,
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

    pub fn get(&self, request: Request) -> Option<&BufferBlock> {
        ut_ad!(request.mtr.get_mtr_state() == MtrState::MTR_ACTIVE);

        let block_opt =
            self.page_hash.get(&request.page_id).map(|block| block.as_ref());

        let block = block_opt.unwrap();

        match request.rw_latch {
            ReadWriteLatch::RW_S_LATCH => {
                let _unused = block.lock.read().unwrap();
            }
            ReadWriteLatch::RW_X_LATCH => {
                let _unused = block.lock.write().unwrap();
            }
            ReadWriteLatch::RW_NO_LATCH => {
                // The caller does not need any latch.
            }
        }

        None
    }

    pub fn hash_get_page(
        &self,
        space_id: SpaceId,
        page_no: PageNo,
    ) -> Option<&BufferBlock> {
        let page_id = PageId { space_id, page_no };
        self.page_hash.get(&page_id).map(|block| block.as_ref())
    }

    /// Check if the page is present in the hashmap.
    pub fn peek(&self, space_id: SpaceId, page_no: PageNo) -> bool {
        let page_id = PageId { space_id, page_no };
        self.page_hash.contains_key(&page_id)
    }

    pub fn io_complete(&self, buf_page: BufPage) {}
}
