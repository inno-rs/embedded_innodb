use std::cmp::Ordering;

use crate::data::data0types::DATA_FLOAT;
use crate::dict::dict0types::Index;
use crate::innodb::innodb0types::UNIV_SQL_NULL;
use crate::rem::rem0rec::{REC_INFO_MIN_REC_FLAG, Record};

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
    a: *const u8,
    a_length: usize,
    b: *const u8,
    b_length: usize,
) -> Ordering {
    match m_type {
        DATA_DECIMAL => {
            todo!("Handle decimal comparison");
        }
        DATA_FLOAT => {
            let d1 = unsafe { *(a as *const f32) };
            let d2 = unsafe { *(b as *const f32) };
            return d1.total_cmp(&d2);
        }

        DATA_DOUBLE => {
            let d1 = unsafe { *(a as *const f64) };
            let d2 = unsafe { *(b as *const f64) };
            return d1.total_cmp(&d2);
        }

        _ => {
            // Handle other types
            panic!("Unhandled type");
        }
    }

    Ordering::Less
}

/// Compare two physical records. Only the common first fields are compared.
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
fn cmp_rec_rec(rec_1: Record, rec_2: Record, index: &Index) -> Ordering {
    let rec1_n_fields = rec_1.rec_get_n_fields();
    let rec2_n_fields = rec_2.rec_get_n_fields();

    let mut cur_field = 0;
    while (cur_field < rec1_n_fields) && (cur_field < rec2_n_fields) {
        let col = index.get_nth_col(0);
        let m_type = col.lock().unwrap().d_type.m_type;
        let pr_type = col.lock().unwrap().d_type.pr_type;

        let (_, rec1_f_len) = rec_1.rec_get_nth_field_offs(cur_field);
        let (_, rec2_f_len) = rec_2.rec_get_nth_field_offs(cur_field);

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

        if m_type == DATA_FLOAT as u16 {
            let ret = cmp_whole_field(
                m_type as usize,
                pr_type,
                rec_1.get_ptr(),
                rec1_f_len,
                rec_2.get_ptr(),
                rec2_f_len,
            );
            if ret != Ordering::Equal {
                return ret;
            }

            cur_field += 1;
            continue;
        }
    }

    Ordering::Less
}
