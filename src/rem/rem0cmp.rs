use std::cmp::Ordering;

use crate::{
    data::data0types::{DATA_FLOAT, DTuple},
    dict::dict0types::Index,
    innodb::innodb0types::UNIV_SQL_NULL,
    rem::rem0rec::{REC_INFO_MIN_REC_FLAG, Record},
    some_or_panic,
    ut::ut0dbg::ut_ad,
    ut_panic,
};

/// Compare two data fields for which the data types are such that we must compare whole fields.
///
/// # Arguments
///
/// * `m_type` - The type of the first field.
/// * `pr_type` - The type of the second field.
/// * `a` - Pointer to the first field.
/// * `a_length` - Length of the first field.
/// * `b` - Pointer to the second field.
/// * `b_length` - Length of the second field.
///     
/// # Returns
///
/// * `Ordering::Less` if the first field is less than the second field.
/// * `Ordering::Greater` if the first field is greater than the second field.
/// * `Ordering::Equal` if the fields are equal.
///3
fn cmp_whole_field(
    m_type: usize,
    pr_type: usize,
    a: &[u8],
    b: &[u8],
) -> Ordering {
    match m_type {
        DATA_FLOAT => {
            let d1 = f32::from_be_bytes(a.try_into().unwrap());
            let d2 = f32::from_be_bytes(b.try_into().unwrap());
            d1.total_cmp(&d2)
        }

        DATA_DOUBLE => {
            let d1 = f64::from_be_bytes(a.try_into().unwrap());
            let d2 = f64::from_be_bytes(b.try_into().unwrap());
            d1.total_cmp(&d2)
        }

        DATA_DECIMAL => {
            ut_panic!("Handle decimal comparison")
        }

        _ => {
            // Handle other types
            ut_panic!("Unhandled type");
        }
    }
}

/// Compare two physical records. Only the common first fields are compared.
///
/// # Arguments
/// * `rec_1` - The first record.
/// * `rec_2` - The second record.
/// * `index` - The index to which the records belong.
/// * `matched_fields` - The number of completely matched fields.
/// * `matched_bytes` - The number of bytes which are already matching for the current field.
///
/// # Returns
///
/// * `Ordering::Less` if the first record is less than the second record.
/// * `Ordering::Greater` if the first record is greater than the second record.
/// * `Ordering::Equal` if the records are equal.
pub fn cmp_rec_rec_with_match(
    rec_1: Record,
    rec_2: Record,
    index: &Index,
    matched_fields: usize,
    matched_bytes: usize,
) -> Ordering {
    let rec1_n_fields = rec_1.rec_get_n_fields();
    let rec2_n_fields = rec_2.rec_get_n_fields();

    let mut cur_field = matched_fields;
    let mut cur_bytes = matched_bytes;
    while (cur_field < rec1_n_fields) && (cur_field < rec2_n_fields) {
        let col = index.get_nth_col(cur_field);

        let m_type = col.lock().unwrap().dtype.m_type;
        let pr_type = col.lock().unwrap().dtype.pr_type;

        let (_, rec1_f_len) = rec_1.rec_get_nth_field_offs(cur_field);
        let (_, rec2_f_len) = rec_2.rec_get_nth_field_offs(cur_field);

        if cur_bytes == 0 {
            if cur_field == 0 {
                let rec1_info_bits = rec_1.rec_get_info_bits();
                let rec2_info_bits = rec_2.rec_get_info_bits();

                if (rec1_info_bits & REC_INFO_MIN_REC_FLAG)
                    == REC_INFO_MIN_REC_FLAG
                {
                    if (rec2_info_bits & REC_INFO_MIN_REC_FLAG)
                        != REC_INFO_MIN_REC_FLAG
                    {
                        return Ordering::Less;
                    }
                } else if (rec2_info_bits & REC_INFO_MIN_REC_FLAG)
                    == REC_INFO_MIN_REC_FLAG
                {
                    return Ordering::Greater;
                }
            }

            if rec_1.rec_nth_external(cur_field)
                || rec_2.rec_nth_external(cur_field)
            {
                return Ordering::Equal;
            }

            if rec1_f_len == UNIV_SQL_NULL || rec2_f_len == UNIV_SQL_NULL {
                if rec1_f_len == rec2_f_len {
                    // implement next_field
                    cur_field += 1;
                    continue;
                } else if rec1_f_len == UNIV_SQL_NULL {
                    /* We define the SQL null to be the smallest possible value of a field in the alphabetical order. */
                    return Ordering::Less;
                } else {
                    return Ordering::Greater;
                }
            }
        }

        let rec1_data =
            some_or_panic!(rec_1.rec_get_data(cur_field)).data_or_panic();
        let rec2_data =
            some_or_panic!(rec_2.rec_get_data(cur_field)).data_or_panic();

        if m_type == DATA_FLOAT as u16 {
            let ret = cmp_whole_field(
                m_type as usize,
                pr_type,
                rec1_data.as_bytes(),
                rec2_data.as_bytes(),
            );
            if ret != Ordering::Equal {
                return ret;
            }

            cur_field += 1;
            continue;
        }

        loop {
            if cur_bytes >= rec1_f_len && cur_bytes < rec2_f_len {
                return Ordering::Less;
            } else if cur_bytes >= rec2_f_len && cur_bytes < rec1_f_len {
                return Ordering::Greater;
            }

            if rec2_f_len <= cur_bytes || rec1_f_len <= cur_bytes {
                cur_field += 1;
                cur_bytes = 0;

                // We have matched everything till now, but rec1 has more fields.
                if cur_field < rec1_n_fields && cur_field > rec2_n_fields {
                    return Ordering::Greater;
                } else if cur_field > rec1_n_fields
                    && cur_field < rec2_n_fields
                {
                    return Ordering::Greater;
                } else if cur_field == rec1_n_fields
                    && cur_field == rec2_n_fields
                {
                    return Ordering::Equal;
                }
            }

            let rec1_byte =
                rec_1.get_nth_field_byte_at_offset(cur_field, cur_bytes);
            let rec2_byte =
                rec_2.get_nth_field_byte_at_offset(cur_field, cur_bytes);

            if rec1_byte < rec2_byte {
                return Ordering::Less;
            } else if rec1_byte > rec2_byte {
                return Ordering::Greater;
            } else {
                cur_bytes += 1;
            }
        }
    }

    Ordering::Less
}

/// Compare two physical records.
/// Only the common first fields are compared.
///
/// # Arguments
/// * `rec_1` - The first record.
/// * `rec_2` - The second record.
/// * `index` - The index to which the records belong.
///
/// # Returns
///
/// * `Ordering::Less` if the first record is less than the second record.
/// * `Ordering::Greater` if the first record is greater than the second record.
/// * `Ordering::Equal` if the records are equal.
pub fn cmp_rec_rec(rec_1: Record, rec_2: Record, index: &Index) -> Ordering {
    cmp_rec_rec_with_match(rec_1, rec_2, index, 0, 0)
}

/// Compare a data tuple to a physical record.
/// Only dtuple.n_fields_cmp are used for comparison.
/// If rec has some externally stored fields,
/// we do not compare them but return Ordering::Equal if such a comparison is made.
///
/// # Arguments
/// * `dtuple` - The data tuple to compare.
/// * `rec` - The physical record to compare.
///
/// # Returns
///
/// * `Ordering::Less` if the data tuple is less than the record.
/// * `Ordering::Greater` if the data tuple is greater than the record.
/// * `Ordering::Equal` if they are equal.
pub fn cmp_dtuple_rec(dtuple: &DTuple, rec: Record) -> Ordering {
    cmp_dtuple_rec_with_match(dtuple, rec, &mut 0, &mut 0)
}

/// Compare a data tuple to a physical record.
/// Only dtuple.n_fields_cmp are used for comparison.
/// If rec has some externally stored fields,
/// we do not compare then but return Ordering::Equal if such a comparison is made.
pub fn cmp_dtuple_rec_with_match(
    dtuple: &DTuple,
    rec: Record,
    matched_fields: &mut usize,
    matched_bytes: &mut usize,
) -> Ordering {
    let rec_n_fields = rec.rec_get_n_fields();
    let dtuple_n_fields = dtuple.dtuple_get_n_fields_cmp();

    let mut cur_field = *matched_fields;
    let mut cur_bytes = *matched_bytes;

    ut_ad!(cur_field < rec_n_fields);
    ut_ad!(cur_field < dtuple_n_fields);

    // 1. the first step is to check if one of them is set to min rec flag.
    if cur_field == 0 && cur_bytes == 0 {
        let rec_info = rec.rec_get_info_bits();
        let tup_info = dtuple.dtuple_get_info_bits();

        if rec_info & REC_INFO_MIN_REC_FLAG == REC_INFO_MIN_REC_FLAG {
            return Ordering::Less;
        } else if tup_info & REC_INFO_MIN_REC_FLAG == REC_INFO_MIN_REC_FLAG {
            return Ordering::Greater;
        }
    }

    // Match fields in a loop. We stop if we run out of fields in dtuple or find an externally stored field.

    while cur_field < dtuple_n_fields {
        // If we have matched 0 bytes, it may be the case that both fields are SQL NULL, or the record or the dtuple may be a predefined minimum.

        let dtuple_field = dtuple.dtuple_get_nth_field(cur_field);
        let dtuple_f_len = dtuple_field.dfield_get_len();

        let (_rec_field_offset, rec_f_len) = rec.rec_get_nth_field(cur_field);

        if cur_bytes == 0 {
            if rec.rec_nth_external(cur_field) {
                // We do not compare the externally stored field.
                return Ordering::Equal;
            }

            if dtuple_f_len == UNIV_SQL_NULL {
                if rec_f_len == UNIV_SQL_NULL {
                    // We need to move to the next field.
                    cur_field += 1;
                    cur_bytes = 0;
                } else {
                    return Ordering::Less;
                }
            } else if rec_f_len == UNIV_SQL_NULL {
                return Ordering::Greater;
            }

            let dfield_data = dtuple_field.dfield_get_data();
            let rec_data =
                some_or_panic!(rec.rec_get_data(cur_field)).data_or_panic();

            if dtuple_field.dtype.m_type >= DATA_FLOAT as u16 {
                let ret = cmp_whole_field(
                    dtuple_field.dtype.m_type as usize,
                    dtuple_field.dtype.pr_type,
                    rec_data.as_bytes(),
                    dfield_data.as_slice(),
                );

                if ret != Ordering::Equal {
                    return ret;
                }
            }
        }

        loop {
            if cur_bytes >= dtuple_f_len && cur_bytes < rec_f_len {
                return Ordering::Less;
            } else if cur_bytes >= rec_f_len && cur_bytes < dtuple_f_len {
                return Ordering::Greater;
            }

            if dtuple_f_len <= cur_bytes || rec_f_len <= cur_bytes {
                cur_field += 1;
                cur_bytes = 0;

                // We have matched everything till now, but rec1 has more fields.
                if cur_field < rec_n_fields && cur_field > dtuple_n_fields {
                    return Ordering::Greater;
                } else if cur_field > rec_n_fields
                    && cur_field < dtuple_n_fields
                {
                    return Ordering::Less;
                } else if cur_field == rec_n_fields
                    && cur_field == dtuple_n_fields
                {
                    return Ordering::Equal;
                }
            }

            let rec1_byte =
                rec.get_nth_field_byte_at_offset(cur_field, cur_bytes);
            let dtuple_byte = *dtuple
                .dtuple_get_nth_field(cur_field)
                .dfield_get_data()
                .as_slice()
                .get(cur_bytes)
                .unwrap();

            if dtuple_byte < rec1_byte {
                return Ordering::Less;
            } else if dtuple_byte > rec1_byte {
                return Ordering::Greater;
            } else {
                cur_bytes += 1;
            }
        }
    }

    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use crate::{
        data::data0types::{DATA_DOUBLE, DATA_FLOAT},
        rem::rem0cmp::cmp_whole_field,
    };

    #[test]
    fn test_cmp_whole_field_float32() {
        {
            let f1 = 34.5f32.to_le_bytes();
            let f2 = 14.3f32.to_le_bytes();

            let cmp = cmp_whole_field(DATA_FLOAT, 0, &f1, &f2);
            assert_eq!(cmp, Ordering::Greater);
        }

        {
            let f1 = 14.3f32.to_le_bytes();
            let f2 = 34.5f32.to_le_bytes();

            let cmp = cmp_whole_field(DATA_FLOAT, 0, &f1, &f2);
            assert_eq!(cmp, Ordering::Less);
        }

        {
            let f1 = 14.3f32.to_le_bytes();

            let cmp = cmp_whole_field(DATA_FLOAT, 0, &f1, &f1);
            assert_eq!(cmp, Ordering::Equal);
        }
    }

    #[test]
    fn test_cmp_whole_field_float64() {
        {
            let f1 = 34.5f64.to_le_bytes();
            let f2 = 14.3f64.to_le_bytes();

            let cmp = cmp_whole_field(DATA_DOUBLE, 0, &f1, &f2);
            assert_eq!(cmp, Ordering::Greater);
        }

        {
            let f1 = 14.3f64.to_le_bytes();
            let f2 = 34.5f64.to_le_bytes();

            let cmp = cmp_whole_field(DATA_DOUBLE, 0, &f1, &f2);
            assert_eq!(cmp, Ordering::Less);
        }

        {
            let f1 = 14.3f64.to_le_bytes();

            let cmp = cmp_whole_field(DATA_DOUBLE, 0, &f1, &f1);
            assert_eq!(cmp, Ordering::Equal);
        }
    }
}
