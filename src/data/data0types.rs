use derive_builder::Builder;

/** Flags ORed to the precise data type */

/** This is ORed to the precise type when the column is declared as NOT nullptr. */
pub const DATA_NOT_NULL: usize = 256;

/* The 'MAIN TYPE' of a column */

/// Character varying of the latin1_swedish_ci charset-collection.
pub const DATA_VARCHAR: usize = 1;

/// Fixed-length character of the latin1_swedish_ci charset-collection.
pub const DATA_CHAR: usize = 2;

/// Binary string of fixed length.
pub const DATA_FIXBINARY: usize = 3;

/// Binary string of variable length.
pub const DATA_BINARY: usize = 4;

/// Binary large object, or a TEXT type.
pub const DATA_BLOB: usize = 5;

/// Integer can be any size 1-8 bytes.
pub const DATA_INT: usize = 6;

/// Address of the child page in the node pointer.
pub const DATA_SYS_CHILD: usize = 7;

/// System Column.
pub const DATA_SYS: usize = 8;

//// Data types >= DATA_FLOAT must be compared using the whole field, not as binary string.
pub const DATA_FLOAT: usize = 9;
pub const DATA_DOUBLE: usize = 10;

/// Decimal number stored as an ASCII string.
pub const DATA_DECIMAL: usize = 11;

/** English language character string: only used for InnoDB's own system tables */
pub const DATA_ENGLISH: usize = 4;

#[derive(Clone)]
pub struct DType {
    /// Main data type.
    pub m_type: u8,
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
#[derive(Builder)]
pub struct DTuple {
    /// Info bits of an index record: the default is 0;
    /// this field is used if an index record is built from a data tuple.
    pub info_bits: usize,
    /// Number of fields in the dtuple.
    pub n_fields: usize,
    /// Fields of the dtuple.
    #[builder(default, setter(each = "add_field"))]
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
