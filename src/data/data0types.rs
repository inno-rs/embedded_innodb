use derive_builder::Builder;
/*
The 'MAIN TYPE' of a column
+------+-------------------+--------------------------------------------+
| Code | Macro             | Meaning                                    |
+------+-------------------+--------------------------------------------+
|  1   | DATA_VARCHAR      | Variable-length character string           |
|  2   | DATA_CHAR         | Fixed-length character string              |
|  3   | DATA_FIXBINARY    | Fixed-length binary string                 |
|  4   | DATA_BINARY       | Variable-length binary string              |
|  5   | DATA_BLOB         | Large binary object (BLOB, TEXT, etc.)     |
|  6   | DATA_INT          | Integer (1–8 bytes)                        |
|  7   | DATA_SYS_CHILD    | Pointer to child page (non-leaf node)      |
|  8   | DATA_SYS          | System column (e.g., trx_id, roll_ptr)     |
|  9   | DATA_FLOAT        | Single-precision floating point             |
| 10   | DATA_DOUBLE       | Double-precision floating point             |
| 11   | DATA_DECIMAL      | ASCII string-encoded decimal value         |
| 12   | DATA_VARCLIENT    | Variable-length char for client use        |
| 13   | DATA_CLIENT       | Charset string (any char/varchar variant)  |
| 63   | DATA_MTYPE_MAX    | Maximum value used for mtype               |
+------+-------------------+--------------------------------------------+
*/

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

pub const DATA_VARCLIENT: usize = 12;

pub const DATA_CLIENT: usize = 13;

pub const DATA_MTYPE_MAX: usize = 63;

/// Precise data types

/// Row id.
pub const DATA_ROW_ID: usize = 0;
pub const DATA_ROW_ID_LEN: usize = 6;

/// Transaction id.
pub const DATA_TRX_ID: usize = 1;
pub const DATA_TRX_ID_LEN: usize = 6;

/// Rollback pointer.
pub const DATA_ROLL_PTR: usize = 2;
pub const DATA_ROLL_PTR_LEN: usize = 7;

/// Number of sys columns
pub const DATA_SYS_COLS: usize = 3;

/** Flags ORed to the precise data type */

/** This is ORed to the precise type when the column is declared as NOT nullptr. */
pub const DATA_NOT_NULL: usize = 256;

/// Unsigned integer
pub const DATA_UNSIGNED: usize = 512;

/// Binary string
pub const DATA_BINARY_TYPE: usize = 1024;

/// Data custom type
pub const DATA_CUSTOM_TYPE: usize = 2048;

/** English language character string: only used for InnoDB's own system tables */
pub const DATA_ENGLISH: usize = 4;

pub const UNIV_SMALLEST_REC_DATA: &str = "UNIVERSAL SMALLEST RECORD";

/*
┌────────────────────────────────────────────────────┐
│                    dtype_t                         │
├──────────────┬─────────────────────────────────────┤
│  Field       │  Role / Controls                    │
├──────────────┼─────────────────────────────────────┤
│ m_type       │  ▸ Storage **category**              │
│              │     - DATA_INT, DATA_VARCHAR, etc.  │
│              │  ▸ Selects encoding layout          │
│              │  ▸ Affects variable vs fixed        │
├──────────────┼─────────────────────────────────────┤
│ pr_type      │  ▸ **Flags and attributes**          │
│              │     - Nullable (DATA_NOT_NULL)      │
│              │     - Unsigned (DATA_UNSIGNED)      │
│              │     - Charset/binary (DATA_BINARY)  │
│              │     - MySQL type ID (<< 24)         │
│              │  ▸ Affects indexing, comparison     │
├──────────────┼─────────────────────────────────────┤
│ len          │  ▸ Max **length in bytes**           │
│              │  ▸ For fixed: exact size            │
│              │  ▸ For varchar/blob: max byte size  │
└──────────────┴─────────────────────────────────────┘

Example:

Column: VARCHAR(255) CHARACTER SET utf8mb4 NOT NULL

m_type   = DATA_VARCHAR
pr_type  = DATA_NOT_NULL | DATA_MBMINMAXLEN | (4 << 8) | (MYSQL_TYPE_VARCHAR << 24)
len     = 1020  (255 × 4 bytes for utf8mb4)


Column Definition → dtype_t → Record Format
   │                 │            │
   ├── CHAR(10) ─────┘            └── 10 bytes, fixed, no prefix
   ├── VARCHAR(500)              → 1–2 byte length prefix + data
   ├── INT UNSIGNED NOT NULL     → 4 bytes, unsigned, no null bit
   └── TEXT                      → 20-byte external pointer
*/

#[derive(Clone, Builder, Debug)]
pub struct DType {
    /// Main data type.
    pub m_type: u16,
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
#[derive(Builder, Debug)]
pub struct DTuple {
    /// Info bits of an index record: the default is 0;
    /// this field is used if an index record is built from a data tuple.
    pub info_bits: usize,
    /// Number of fields in the dtuple.
    pub n_fields: usize,
    /// Number of fields that should be used during comparison functions used in rec0cmp.
    /// Index search is performed by comparing only these fields and the others are ignored.
    /// The default value in DTuple creation is the same value as the number of fields.
    pub n_fields_cmp: usize,
    /// Fields of the dtuple.
    #[builder(default, setter(each = "add_field"))]
    pub fields: Vec<DField>,
}

/// Structure for an SQL data field.
#[derive(Clone, Builder, Debug)]
pub struct DField {
    /// Pointer to the data of the field.
    pub data: Vec<u8>,
    /// Data length; UNIV_SQL_NULL if SQL null.
    pub len: usize,
    /// true=externally stored, false=local.
    #[builder(default = false)]
    pub ext: bool,
    /// Type of data.
    pub dtype: DType,
}

pub struct BigRecField {
    pub field_no: usize,
    pub len: usize,
    pub data: Vec<u8>,
}

pub struct BigRec {
    pub n_fields: usize,
    pub fields: Vec<BigRecField>,
}

// Compile-time constants for each DType
pub const DTYPE_VARCHAR: DType = DType {
    m_type: DATA_VARCHAR as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_CHAR: DType = DType {
    m_type: DATA_CHAR as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_FIXBINARY: DType = DType {
    m_type: DATA_FIXBINARY as u16,
    pr_type: DATA_BINARY_TYPE,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_BINARY: DType = DType {
    m_type: DATA_BINARY as u16,
    pr_type: DATA_BINARY_TYPE,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_BLOB: DType = DType {
    m_type: DATA_BLOB as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_INT: DType = DType {
    m_type: DATA_INT as u16,
    pr_type: 0,
    len: 4,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_INT_UNSIGNED: DType = DType {
    m_type: DATA_INT as u16,
    pr_type: DATA_UNSIGNED,
    len: 8,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_SYS_CHILD: DType = DType {
    m_type: DATA_SYS_CHILD as u16,
    pr_type: 0,
    len: 4,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_SYS: DType = DType {
    m_type: DATA_SYS as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_FLOAT: DType = DType {
    m_type: DATA_FLOAT as u16,
    pr_type: 0,
    len: 4,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_DOUBLE: DType = DType {
    m_type: DATA_DOUBLE as u16,
    pr_type: 0,
    len: 8,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_DECIMAL: DType = DType {
    m_type: DATA_DECIMAL as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_VARCLIENT: DType = DType {
    m_type: DATA_VARCLIENT as u16,
    pr_type: 0,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};

pub const DTYPE_CLIENT: DType = DType {
    m_type: DATA_CLIENT as u16,
    pr_type: DATA_BINARY_TYPE,
    len: 0,
    mb_min_len: 0,
    mb_max_len: 0,
};
