/*#[cfg(test)]
mod tests {

    use crate::mach::mach0data::mach_write_compressed;
    use crate::mtr::mtr0log::MLog;
    use crate::page::page0page::Page;
    use crate::innodb::innodb0types::{PageFrame, UNIV_PAGE_SIZE};

    use crate::mtr::mtr0mtr::Mtr;
    use crate::mtr::mtr0types::MlogType;
    /// Helper to create a Page backed by a Vec<u8>.
    fn make_page(size: usize) -> (Vec<u8>, Page) {
        // 1) allocate the backing storage
        let mut data = vec![0u8; UNIV_PAGE_SIZE];
        // 2) wrap it in a Page without making `ptr` public
        let page = Page::from_buffer(&mut data);
        (data, page)
    }

    fn setup() -> (MLog, Vec<u8>, Page, Mtr) {
        let mlog = MLog::new();
        let (page_bytes, page) = make_page(64);
        let mut mtr = Mtr::new();
        mtr.start();
        (mlog, page_bytes, page, mtr)
    }

    #[test]
    fn test_mlog_write_u32_1byte() {
        let (mut mlog, mut buf, page, mut mtr) = setup();
        let offset = 5;
        let val = 0xAB; // fits in 1 byte

        // Perform the write+log
        mlog.mlog_write_u32(&page, offset, val, MlogType::MLOG_1BYTE, &mut mtr);

        // 1) Page updated
        assert_eq!(buf[offset], val as u8);

        // 2) One record counted
        assert_eq!(mtr.get_n_log_records(), 1);

        // 3) Strip off the header using your parser
        let (rec_type, _space_id, _page_no, rem) =
            mlog.mlog_parse_initial_log_record(&mlog.buf).expect("header parse");
        assert_eq!(rec_type, MlogType::MLOG_1BYTE);

        // 4) Now `rem` starts at the 2-byte in-page offset:
        let logged_off = u16::from_le_bytes([rem[0], rem[1]]) as usize;
        assert_eq!(logged_off, offset);

        // 5) Then the 4-byte value:
        let logged_val = u32::from_le_bytes(rem[2..6].try_into().unwrap());
        assert_eq!(logged_val, val as u32);

        // 6) No extra bytes after that
        assert_eq!(&rem[6..], &[] as &[u8]);
    }

    #[test]
    fn test_mlog_write_u32_2bytes() {
        let (mut mlog, mut buf, page, mut mtr) = setup();
        let offset = 10;
        let val = 0xBEEF; // fits in 2 bytes
        mlog.mlog_write_u32(&page, offset, val, MlogType::MLOG_2BYTES, &mut mtr);

        // Page update
        let read = u16::from_le_bytes([buf[offset], buf[offset+1]]);
        assert_eq!(read, val as u16);

        // Log
        let b = &mlog.buf; let mut i = 0;
        assert_eq!(b[i], MlogType::MLOG_2BYTES as u8); i += 1;
        let off = u16::from_le_bytes([b[i], b[i+1]]); i += 2;
        assert_eq!(off as usize, offset);
        let v = u32::from_le_bytes(b[i..i+4].try_into().unwrap()); i += 4;
        assert_eq!(v, val as u32);
        assert_eq!(i, b.len());
        assert_eq!(mtr.get_n_log_records(), 1);
    }

    #[test]
    fn test_mlog_write_u32_4bytes() {
        let (mut mlog, mut buf, page, mut mtr) = setup();
        let offset = 20;
        let val = 0xDEAD_BEEF;
        mlog.mlog_write_u32(&page, offset, val, MlogType::MLOG_4BYTES, &mut mtr);

        // Page
        let got = u32::from_le_bytes(buf[offset..offset+4].try_into().unwrap());
        assert_eq!(got, val);

        // Log
        let b = &mlog.buf; let mut i = 0;
        assert_eq!(b[i], MlogType::MLOG_4BYTES as u8); i += 1;
        let off = u16::from_le_bytes([b[i], b[i+1]]); i += 2;
        assert_eq!(off as usize, offset);
        let v = u32::from_le_bytes(b[i..i+4].try_into().unwrap()); i += 4;
        assert_eq!(v, val);
        assert_eq!(i, b.len());
        assert_eq!(mtr.get_n_log_records(), 1);
    }

    #[test]
    fn test_mlog_write_uint64() {
        let (mut mlog, mut buf, page, mut mtr) = setup();
        let offset = 16;
        let val = 0x1122_3344_5566_7788u64;
        mlog.mlog_write_uint64(&page, offset, val, &mut mtr);

        // Page (LE)
        assert_eq!(&buf[offset..offset+8], &val.to_le_bytes());

        // One record
        assert_eq!(mtr.get_n_log_records(), 1);

        // Log buffer
        let b = &mlog.buf; let mut i = 0;
        assert_eq!(b[i], MlogType::MLOG_8BYTES as u8); i += 1;
        // in-page offset
        let off = u16::from_le_bytes([b[i], b[i+1]]); i += 2;
        assert_eq!(off as usize, offset);
        // compressed high 32
        let high = (val >> 32) as u32;
        let mut tmp = [0u8; 5];
        let n = mach_write_compressed(&mut tmp, high);
        assert_eq!(&b[i..i+n], &tmp[..n]);
        i += n;
        // low 32
        let low = u32::from_le_bytes(b[i..i+4].try_into().unwrap());
        assert_eq!(low as u64, (val & 0xFFFF_FFFF) as u64);
        i += 4;
        assert_eq!(i, b.len());
    }

    #[test]
    #[test]
    fn test_mlog_write_string_and_parse() {
        let (mut mlog, mut buf, page, mut mtr) = setup();
        let offset = 12;
        let data = b"rustacean";

        // 1) Write + log
        mlog.mlog_write_string(&page, offset, data, &mut mtr);
        assert_eq!(&buf[offset..offset + data.len()], data);
        assert_eq!(mtr.get_n_log_records(), 1);
        let payload_start = 11 + 2 + 2;

        // 2) Extract just the 4+len bytes we want to parse
        let start = payload_start - 4;
        let end   = start + 4 + data.len();
        // clone into a fresh Vec so we don't borrow `mlog` at all
        let record: Vec<u8> = mlog.buf[start..end].to_vec();

        // 3) Now we can call parse_string without any conflicting borrow
        let mut page_copy = buf.clone();
        let rest = mlog
            .mlog_parse_string(&record, Some(&mut page_copy[..]))
            .unwrap();

        // 4) Validate
        assert_eq!(&page_copy[offset..offset + data.len()], data);
        assert_eq!(rest, &[]);
    }
}
*/
