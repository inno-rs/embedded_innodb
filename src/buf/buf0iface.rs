use crate::innodb::innodb0types::PageId;
use crate::mtr::mtr0types::FixMode;

pub trait BufPoolLike {
    fn release_page(&self, page_id: PageId, mode: FixMode);
}