


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fut::fut0lst::*;

    #[test]
    fn test_filaddr_read_and_null() {
        // Prepare a buffer with a FilAddr at offset 5:
        // page_no = 0x0A0B0C0D, byte_off = 0x1122
        let mut buf = vec![0u8; 5 + FIL_ADDR_SIZE];
        let page_no: u32 = 0x0A0B0C0D;
        let byte_off: u16 = 0x1122;
        buf[5 + FIL_ADDR_PAGE .. 5 + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&page_no.to_le_bytes());
        buf[5 + FIL_ADDR_BYTE .. 5 + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&byte_off.to_le_bytes());

        let addr = FilAddr::read(&buf, 5);
        assert_eq!(addr.page_no, page_no);
        assert_eq!(addr.byte_off, byte_off);

        // The NULL constant should be all-0xFF in its page_no
        assert_eq!(FilAddr::NULL.page_no, u32::MAX);
        assert_eq!(FilAddr::NULL.byte_off, 0);
    }

    #[test]
    fn test_filst_len_first_last() {
        // Base-node is 16 bytes: [len(4)][first(6)][last(6)]
        let mut base = vec![0u8; FLST_LAST + FIL_ADDR_SIZE];
        // Set length = 0xDEADBEEF
        let len_val: u32 = 0xDEADBEEF;
        base[FLST_LEN .. FLST_LEN + 4].copy_from_slice(&len_val.to_le_bytes());
        assert_eq!(FilAddr::len(&base), len_val as usize);

        // Set FIRST pointer to (0x11121314, 0x2233)
        let first = FilAddr { page_no: 0x11121314, byte_off: 0x2233 };
        base[FLST_FIRST + FIL_ADDR_PAGE .. FLST_FIRST + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&first.page_no.to_le_bytes());
        base[FLST_FIRST + FIL_ADDR_BYTE .. FLST_FIRST + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&first.byte_off.to_le_bytes());
        assert_eq!(FilAddr::first(&base), first);

        // Set LAST pointer to (0x55667788, 0x3344)
        let last = FilAddr { page_no: 0x55667788, byte_off: 0x3344 };
        base[FLST_LAST + FIL_ADDR_PAGE .. FLST_LAST + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&last.page_no.to_le_bytes());
        base[FLST_LAST + FIL_ADDR_BYTE .. FLST_LAST + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&last.byte_off.to_le_bytes());
        assert_eq!(FilAddr::last(&base), last);
    }

    #[test]
    fn test_filst_prev_next() {
        // List-node is 12 bytes: [prev(6)][next(6)]
        let mut node = vec![0u8; FIL_ADDR_SIZE * 2];
        // Set PREV = (0x01020304, 0x0506)
        let prev = FilAddr { page_no: 0x01020304, byte_off: 0x0506 };
        node[FLST_PREV + FIL_ADDR_PAGE .. FLST_PREV + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&prev.page_no.to_le_bytes());
        node[FLST_PREV + FIL_ADDR_BYTE .. FLST_PREV + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&prev.byte_off.to_le_bytes());
        assert_eq!(FilAddr::prev(&node), prev);

        // Set NEXT = (0x0A0B0C0D, 0x0E0F)
        let next = FilAddr { page_no: 0x0A0B0C0D, byte_off: 0x0E0F };
        node[FLST_NEXT + FIL_ADDR_PAGE .. FLST_NEXT + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&next.page_no.to_le_bytes());
        node[FLST_NEXT + FIL_ADDR_BYTE .. FLST_NEXT + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&next.byte_off.to_le_bytes());
        assert_eq!(FilAddr::next(&node), next);
    }

    #[test]
    fn test_filaddr_write() {
        // make an 8-byte buffer so offset+6 stays in bounds
        let mut buf = vec![0u8; 8];
        // pick a nonzero address
        let addr = FilAddr { page_no: 0x1122_3344, byte_off: 0x5566 };
        // write it at offset 1
        addr.write(&mut buf, 1);

        // check the 4-byte page_no at buf[1..5]
        assert_eq!(&buf[1..5], &0x1122_3344u32.to_le_bytes());

        // check the 2-byte byte_off at buf[5..7]
        assert_eq!(&buf[5..7], &0x5566u16.to_le_bytes());

        // bytes before offset 1 and after offset+6 should be untouched (still 0)
        assert_eq!(buf[0], 0);
        assert_eq!(buf[7], 0);
    }
}
