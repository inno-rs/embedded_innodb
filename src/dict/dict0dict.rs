use std::sync::{Arc, Mutex};

use crate::{btr::btr0btr::BTree, fsp::fsp0fsp::Fsp};

pub struct Dict {
    /// Reference to Fsp.
    fsp: Arc<Mutex<Fsp>>,
    /// Reference to BTree.
    btree: Arc<BTree>,
}

impl Dict {
    pub fn new(fsp: Arc<Mutex<Fsp>>, btree: Arc<BTree>) -> Self {
        Self { fsp, btree }
    }

    pub fn get_fsp(&self) -> Arc<Mutex<Fsp>> {
        self.fsp.clone()
    }

    pub fn get_btree(&self) -> Arc<BTree> {
        self.btree.clone()
    }
}
