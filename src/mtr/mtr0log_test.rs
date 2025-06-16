#[cfg(test)]
mod tests {

    use crate::mach::mach0data::mach_write_compressed;
    use crate::mtr::mtr0log::MLog;
    use crate::mtr::mtr0mtr::Mtr;
    use crate::mtr::mtr0types::MlogType;

    fn setup() -> (MLog, Vec<u8>, Mtr) {
        let mlog = MLog { buf: Vec::new() };
        let page = vec![0u8; 64];
        let mut mtr = Mtr::new();
        mtr.start();
        (mlog, page, mtr)
    }

    #[test]
    fn test_mlog_write_u32_1byte() {
        let (mut mlog, mut page, mut mtr) = setup();
        let offset = 5;
        let val = 0xAB; // fits in 1 byte
        mlog.mlog_write_u32(
            &mut page,
            offset,
            val,
            MlogType::MLOG_1BYTE,
            &mut mtr,
        );

        // Page update
        assert_eq!(page[offset], val as u8);

        // Log buffer: [type][u16 offset BE][u32 value BE]
        let buf = &mlog.buf;
        let mut i = 0;
        assert_eq!(buf[i], MlogType::MLOG_1BYTE as u8);
        i += 1;
        let off = u16::from_le_bytes([buf[i], buf[i + 1]]);
        i += 2;
        assert_eq!(off as usize, offset);
        let logged_val = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        assert_eq!(logged_val, val as u32);
        i += 4;

        // Exactly one record
        assert_eq!(mtr.get_n_log_records(), 1);
        assert_eq!(i, buf.len());
    }

    #[test]
    fn test_mlog_write_u32_2bytes() {
        let (mut mlog, mut page, mut mtr) = setup();
        let offset = 10;
        let val = 0xBEEF; // fits in 2 bytes
        mlog.mlog_write_u32(
            &mut page,
            offset,
            val,
            MlogType::MLOG_2BYTES,
            &mut mtr,
        );

        // Page update
        let high = page[offset];
        let low = page[offset + 1];
        assert_eq!(u16::from_le_bytes([high, low]), val as u16);

        // Log buffer
        let buf = &mlog.buf;
        let mut i = 0;
        assert_eq!(buf[i], MlogType::MLOG_2BYTES as u8);
        i += 1;
        let off = u16::from_le_bytes([buf[i], buf[i + 1]]);
        i += 2;
        assert_eq!(off as usize, offset);
        let logged_val = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        assert_eq!(logged_val, val as u32);
        i += 4;

        assert_eq!(mtr.get_n_log_records(), 1);
        assert_eq!(i, buf.len());
    }

    #[test]
    fn test_mlog_write_u32_4bytes() {
        let (mut mlog, mut page, mut mtr) = setup();
        let offset = 20;
        let val = 0xDEAD_BEEF; // fits in 4 bytes
        mlog.mlog_write_u32(
            &mut page,
            offset,
            val,
            MlogType::MLOG_4BYTES,
            &mut mtr,
        );

        // Page update
        let be = &page[offset..offset + 4];
        assert_eq!(u32::from_le_bytes(be.try_into().unwrap()), val as u32);

        // Log buffer
        let buf = &mlog.buf;
        let mut i = 0;
        assert_eq!(buf[i], MlogType::MLOG_4BYTES as u8);
        i += 1;
        let off = u16::from_le_bytes([buf[i], buf[i + 1]]);
        i += 2;
        assert_eq!(off as usize, offset);
        let logged_val = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        assert_eq!(logged_val, val as u32);
        i += 4;

        assert_eq!(mtr.get_n_log_records(), 1);
        assert_eq!(i, buf.len());
    }

    #[test]
    fn test_mlog_write_uint64() {
        // 1) Prepare a dummy page and MLog/Mtr
        let mut page = vec![0u8; 64];
        let offset = 16;
        let val: usize = 0x1122_3344_5566_7788;
        let mut mlog = MLog { buf: Vec::new() };
        let mut mtr = Mtr::new();
        mtr.start();

        // 2) Invoke the method under test
        mlog.mlog_write_uint64(&mut page, offset, val, &mut mtr);

        // 3) Verify the in-page write (big-endian)
        let expected_page = (val as u64).to_le_bytes();
        assert_eq!(&page[offset..offset + 8], &expected_page);

        // 4) Verify we recorded exactly one log‐record
        assert_eq!(mtr.get_n_log_records(), 1);

        // 5) Inspect the log buffer bytes
        let buf = &mlog.buf;
        let mut idx = 0;

        //   a) First byte = MLOG_8BYTES record type
        assert_eq!(buf[idx], MlogType::MLOG_8BYTES as u8);
        idx += 1;

        //   b) Next two bytes = offset in little-endian
        let off_be = u16::from_le_bytes([buf[idx], buf[idx + 1]]);
        assert_eq!(off_be as usize, offset);
        idx += 2;

        //   c) Next N bytes = compressed high-32 bits
        let high = (val >> 32) as u32;
        let mut tmp = [0u8; 5];
        let n = mach_write_compressed(&mut tmp, high);
        assert_eq!(&buf[idx..idx + n], &tmp[..n]);
        idx += n;

        //   d) Final 4 bytes = low-32 bits in litte-endian
        let low_be = u32::from_le_bytes(buf[idx..idx + 4].try_into().unwrap());
        assert_eq!(low_be as u64, (val as u64) & 0xFFFF_FFFF);
        idx += 4;

        //   e) We consumed the entire buffer
        assert_eq!(idx, buf.len());
    }
}
