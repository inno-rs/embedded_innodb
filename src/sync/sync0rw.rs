#[derive(Eq, PartialEq)]
pub enum ReadWriteLatch {
    RW_S_LATCH = 1,
    RW_X_LATCH = 2,
    RW_NO_LATCH = 3,
}
