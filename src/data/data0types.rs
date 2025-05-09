/** Flags ORed to the precise data type */

/** This is ORed to the precise type when the column is declared as NOT nullptr. */
pub const DATA_NOT_NULL: usize = 256;

/* The 'MAIN TYPE' of a column */

/** Character varying of the latin1_swedish_ci charset-collection */
pub const DATA_VARCHAR: usize = 1;

/** English language character string: only used for InnoDB's own system tables */
pub const DATA_ENGLIGH: usize = 4;

#[derive(Clone)]
pub struct DType {
    /// Main data type.
    pub dtype: u8,
    /// Length of the column in bytes.
    pub len: u16,
    /// Precise type; user data type, charset code,
    /// flags to indicate nullability, signedness, whether this is a binary string
    pub pr_type: usize,
    /// Minimum length of a character in bytes.
    pub mb_min_len: usize,
    /// Maximum length of bytes to store string length.
    pub mb_max_len: usize,
}

/// Structure for an SQL data tuple of fields (logical record).
pub struct DTuple {
    /// Info bits of an index record: the default is 0;
    /// this field is used if an index record is built from a data tuple.
    pub info_bits: usize,
    /// Number of fields in the dtuple.
    pub n_fields: usize,
    /// Fields of the dtuple.
    pub fields: Vec<DField>,
}

/// Structure for an SQL data field.
#[derive(Clone)]
pub struct DField {
    /// Pointer to the data of the field.
    pub data: *const u8,
    /// Data length; UNIV_SQL_NULL if SQL null.
    pub len: usize,
    /// true=externally stored, false=local.
    pub ext: bool,
    /// Type of data.
    pub field_type: DType,
}
