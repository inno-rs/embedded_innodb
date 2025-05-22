use crate::buf::buf0types::BufPool;
use crate::fil::fil0types::SpaceId;
use crate::innodb::innodb0types::PageNo;
use std::sync::Arc;

pub struct BufRea {
    buf_pool: Arc<BufPool>,
}

impl BufRea {
    pub fn new(buf_pool: Arc<BufPool>) -> Self {
        BufRea { buf_pool }
    }

    pub fn buf_read_page(&self, space_id: SpaceId, page_no: PageNo) -> bool {
        true
    }
}
