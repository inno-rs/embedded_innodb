/** The 2-logarithm of UNIV_PAGE_SIZE: */
pub const UNIV_PAGE_SIZE_SHIFT: usize = 14;

/** The universal page size of the database */
pub const UNIV_PAGE_SIZE: usize = 1 << UNIV_PAGE_SIZE_SHIFT;
