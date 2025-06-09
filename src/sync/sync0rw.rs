#[derive(Eq, PartialEq)]
pub enum ReadWriteLatch {
    SLatch = 1,
    XLatch = 2,
    NoLatch = 3,
}
