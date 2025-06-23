use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use crate::{btr::btr0btr::BTree, fsp::fsp0fsp::Fsp};

pub struct Dict {
    /// Reference to Fsp.
    fsp: Arc<Mutex<Fsp>>,
    /// Reference to BTree.
    btree: Rc<RefCell<BTree>>,
}

impl Dict {
    pub fn new(fsp: Arc<Mutex<Fsp>>, btree: Rc<RefCell<BTree>>) -> Self {
        Self { fsp, btree }
    }

    pub fn get_fsp(&self) -> Arc<Mutex<Fsp>> {
        self.fsp.clone()
    }

    pub fn get_btree(&self) -> Rc<RefCell<BTree>> {
        self.btree.clone()
    }
}
