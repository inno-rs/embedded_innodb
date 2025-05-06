/** Flags ORed to the precise data type */

/** This is ORed to the precise type when the column is declared as NOT nullptr. */
pub const DATA_NOT_NULL: usize = 256;

/* The 'MAIN TYPE' of a column */

/** Character varying of the latin1_swedish_ci charset-collection */
pub const DATA_VARCHAR: usize = 1;

/** English language character string: only used for InnoDB's own system tables */
pub const DATA_ENGLIGH: usize = 4;

pub struct DTuple {
    pub(crate) number_of_fields: usize,
}

pub struct DField {}
