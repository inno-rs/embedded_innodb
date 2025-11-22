use crate::buf::buf0iface::BufPoolLike;
use crate::buf::buf0buf::BufPool;
use crate::innodb::innodb0types::PageId;
use crate::mtr::mtr0types::FixMode;

impl BufPoolLike for BufPool {
    fn release_page(&self, page_id: PageId, mode: FixMode) {
        BufPool::release_page(self, page_id, mode)
    }
}