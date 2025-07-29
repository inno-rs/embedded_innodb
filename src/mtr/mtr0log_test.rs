#[cfg(test)]
mod tests {
    use crate::{
        fil::fil0types::{FIL_PAGE_OFFSET, FIL_PAGE_SPACE_ID},
        innodb::innodb0types::UNIV_PAGE_SIZE,
        mach::mach0data::{mach_parse_compressed, mach_write_to_4},
        mtr::{
            mtr0log::MLog,
            mtr0mtr::Mtr,
            mtr0types::{MlogType, MtrLogMode},
        },
        page::page0page::Page,
    };

    fn make_page() -> (Vec<u8>, Page) {
        // allocate a fresh backing Vec
        let mut backing = vec![0u8; UNIV_PAGE_SIZE];
        // wrap its buffer pointer in our Page type
        let space_offset = FIL_PAGE_SPACE_ID;
        let page_offset = FIL_PAGE_OFFSET;
        mach_write_to_4(&mut backing[space_offset..], 0);
        mach_write_to_4(&mut backing[page_offset..], 0);
        let page = Page::new(backing.as_mut_ptr());
        (backing, page)
    }

    fn setup() -> (MLog, Vec<u8>, Page) {
        let (backing, page) = make_page();
        let mlog = MLog::new();
        (mlog, backing, page)
    }

    #[test]
    fn test_mlog_write_u32_1byte() {
        let (mut mlog, backing, page) = setup();
        let offset = 128;
        let val = 0xAB;

        mlog.mlog_write_u32(
            &page,
            offset,
            val,
            MlogType::MLOG_1BYTE,
            MtrLogMode::MTR_LOG_MODE_ALL,
        );

        // in-page byte (always little-endian at page)
        let frame = page.to_page_frame();
        assert_eq!(frame.0[offset], val as u8);

        // parse the log: [type][space varint][page_no varint][offset(2B BE)][value(4B BE)]
        let buf = &mlog.buf;
        assert_eq!(buf[0], MlogType::MLOG_1BYTE as u8);

        // skip type + two varints:
        let (space, n1) = mach_parse_compressed(&buf[1..]).unwrap();
        let (pageno, n2) = mach_parse_compressed(&buf[1 + n1..]).unwrap();
        assert_eq!(space, 0);
        assert_eq!(pageno, 0); // Still failing here

        let pos = 1 + n1 + n2;
        let got_off = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        let got_val =
            u32::from_be_bytes(buf[pos + 2..pos + 6].try_into().unwrap());

        assert_eq!(got_off, offset);
        assert_eq!(got_val, val);
        assert_eq!(buf.len(), pos + 6);
    }

    #[test]
    fn test_mlog_write_u32_2bytes_and_4bytes() {
        for &(offset, val, mlog_type) in &[
            (10, 0xBEEF_u32, MlogType::MLOG_2BYTES),
            (20, 0xDEAD_BEEF_u32, MlogType::MLOG_4BYTES),
        ] {
            let (mut mlog, backing, page) = setup();
            mlog.mlog_write_u32(
                &page,
                offset,
                val,
                mlog_type,
                MtrLogMode::MTR_LOG_MODE_ALL,
            );

            // figure out how many bytes we wrote
            let width = match mlog_type {
                MlogType::MLOG_1BYTE => 1,
                MlogType::MLOG_2BYTES => 2,
                MlogType::MLOG_4BYTES => 4,
                _ => unreachable!(),
            };

            // in-page update (little-endian)
            let frame = page.to_page_frame();
            let slice = &frame.0[offset..offset + width];
            let live = match mlog_type {
                MlogType::MLOG_2BYTES => {
                    u16::from_be_bytes(slice.try_into().unwrap()) as u32
                }
                MlogType::MLOG_4BYTES => {
                    u32::from_be_bytes(slice.try_into().unwrap())
                }
                _ => unreachable!(),
            };
            assert_eq!(live, val);

            // now inspect the redo-log header+body
            let buf = &mlog.buf;
            assert_eq!(buf[0], mlog_type as u8);
            let (space, n1) = mach_parse_compressed(&buf[1..]).unwrap();
            let (pageno, n2) = mach_parse_compressed(&buf[1 + n1..]).unwrap();
            assert_eq!(space, 0);
            assert_eq!(pageno, 0);

            let pos = 1 + n1 + n2;
            let got_off =
                u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
            let got_val =
                u32::from_be_bytes(buf[pos + 2..pos + 6].try_into().unwrap());
            assert_eq!(got_off, offset);
            assert_eq!(got_val, val);
        }
    }

    #[test]
    #[ignore]
    fn test_mlog_write_uint64() {
        let (mut mlog, _, page) = setup();
        let offset = 16;
        let val: u64 = 0x1122_3344_5566_7788;

        mlog.mlog_write_uint64(
            &page,
            offset,
            val,
            MtrLogMode::MTR_LOG_MODE_ALL,
        );

        // parse header
        let buf = &mlog.buf;
        assert_eq!(buf[0], MlogType::MLOG_8BYTES as u8);
        let (space, n1) = mach_parse_compressed(&buf[1..]).unwrap();
        let (page_no, n2) = mach_parse_compressed(&buf[1 + n1..]).unwrap();
        assert_eq!(space, 0);
        assert_eq!(page_no, 0);

        let mut pos = 1 + n1 + n2;
        let off = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
        assert_eq!(off, offset);
        pos += 2;

        // now compressed-high + low‐32
        let (high, hsize) = mach_parse_compressed(&buf[pos..]).unwrap();
        assert_eq!(high as u64, val >> 32);
        pos += hsize;

        let low =
            u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap()) as u64;
        assert_eq!(low, val & 0xFFFF_FFFF);
        pos += 4;

        assert_eq!(pos, buf.len());
    }

    #[test]
    fn test_mlog_write_and_parse_string_roundtrip() {
        let (mut mlog, backing, page) = setup();
        let offset = 13;
        let data = b"InnoDB";

        // write + log
        mlog.mlog_write_string(
            &page,
            offset,
            data,
            MtrLogMode::MTR_LOG_MODE_ALL,
        );

        // grab a copy of the bytes we need
        let full = mlog.buf.clone();
        let (space, n1) = mach_parse_compressed(&full[1..]).unwrap();
        let (pageno, n2) = mach_parse_compressed(&full[1 + n1..]).unwrap();
        let pos = 1 + n1 + n2;
        let slice_len = 4 + data.len();
        let slice = full[pos..pos + slice_len].to_vec();

        // now we can call the &mut method without any conflicting borrows
        let mut apply = backing.clone();
        let rem = mlog.mlog_parse_string(&slice, Some(&mut apply)).unwrap();
        assert!(rem.is_empty());
        assert_eq!(&apply[offset..offset + data.len()], data);
    }

    #[test]
    fn test_open_and_write_index_nonzero() {
        let (mut mlog, _backing, page) = setup();
        // request 5‐byte payload
        let size = 5;
        let payload = mlog
            .mlog_open_and_write_index(
                &page,
                MlogType::MLOG_WRITE_STRING,
                size,
                MtrLogMode::MTR_LOG_MODE_ALL,
            )
            .expect("should get a payload slice");
        assert_eq!(payload.len(), size);

        // fill it
        for i in 0..size {
            payload[i] = (i as u8) + 1;
        }

        // now inspect the log buffer
        let buf = &mlog.buf;
        // first byte = record type
        assert_eq!(buf[0], MlogType::MLOG_WRITE_STRING as u8);

        // skip two varints: space_id + page_no
        let (space, n1) = mach_parse_compressed(&buf[1..]).unwrap();
        let (pageno, n2) = mach_parse_compressed(&buf[1 + n1..]).unwrap();
        assert_eq!(space, 0);
        assert_eq!(pageno, 0);

        let start = 1 + n1 + n2;
        // payload should appear immediately after header
        assert_eq!(&buf[start..start + size], &[1, 2, 3, 4, 5]);
        assert_eq!(buf.len(), start + size);
    }

    #[test]
    #[ignore]
    fn test_initial_fast_header() {
        let (mut mlog, _, page) = setup();
        let start = 0;
        let after = mlog.mlog_write_initial_log_record_fast(
            &page,
            MlogType::MLOG_4BYTES,
            start,
        );
        // type + two varints(0)
        assert_eq!(mlog.buf[0], MlogType::MLOG_4BYTES as u8);
        assert_eq!(mach_parse_compressed(&mlog.buf[1..]).unwrap().0, 0);
        assert_eq!(mach_parse_compressed(&mlog.buf[2..]).unwrap().0, 0);
        assert_eq!(after, 3);
    }

    #[test]
    fn test_mlog_catenate_string_disabled() {
        let mut mlog = MLog::new();
        let page = vec![10u8, 20, 30, 40, 50];
        let mtr = Mtr::new(); // default is MTR_LOG_NONE

        // Try to catenate bytes 1..4 from page into log
        mlog.mlog_catenate_string(&page, 1, 3, MtrLogMode::MTR_LOG_NONE);
        // Since logging was disabled, buf must still be empty
        assert!(mlog.buf.is_empty());
    }

    #[test]
    fn test_mlog_catenate_string_enabled() {
        let mut mlog = MLog::new();
        let page = vec![10u8, 20, 30, 40, 50];

        // Append 3 bytes starting at offset=1
        mlog.mlog_catenate_string(&page, 1, 3, MtrLogMode::MTR_LOG_MODE_ALL);

        // Should have appended exactly [20,30,40]
        assert_eq!(mlog.buf, vec![20u8, 30, 40]);

        // If we do it again, it concatenates again
        mlog.mlog_catenate_string(&page, 2, 2, MtrLogMode::MTR_LOG_MODE_ALL);
        assert_eq!(mlog.buf, vec![20u8, 30, 40, 30, 40]);
    }
}
