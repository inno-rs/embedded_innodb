


#[cfg(test)]
mod tests {
    use std::ptr::write;
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

    #[test]
    fn test_write_first_last() {
        // Make a base-node buffer of exactly the right size
        let mut base = vec![0u8; FLST_LAST + FIL_ADDR_SIZE];
        let first = FilAddr { page_no: 0x01020304, byte_off: 0x0506 };
        let last  = FilAddr { page_no: 0x0A0B0C0D, byte_off: 0x0E0F };

        first.write_first(&mut base);
        last.write_last(&mut base);

        // Reading back via the read methods should yield the same
        assert_eq!(FilAddr::first(&base), first);
        assert_eq!(FilAddr::last(&base),  last);

        // Ensure nothing else clobbered
        for i in 0..FLST_FIRST {
            assert_eq!(base[i], 0);
        }
        for i in FLST_FIRST+FIL_ADDR_SIZE..FLST_LAST {
            assert_eq!(base[i], 0);
        }
        for i in FLST_LAST+FIL_ADDR_SIZE..base.len() {
            assert_eq!(base[i], 0);
        }
    }

    #[test]
    fn test_write_prev_next() {
        // List-node buffer of exactly 12 bytes
        let mut node = vec![0xAAu8; FIL_ADDR_SIZE * 2];
        let prev = FilAddr { page_no: 0x40414243, byte_off: 0x4455 };
        let next = FilAddr { page_no: 0x60616263, byte_off: 0x6475 };

        prev.write_prev(&mut node);
        next.write_next(&mut node);


        // Read back
        assert_eq!(FilAddr::prev(&node), prev);
        assert_eq!(FilAddr::next(&node), next);

        // Check that bytes outside prev/next were untouched
        // prev occupies 0..6, next 6..12, so nothing else to check here
        // but we can assert they didn’t revert to 0:
        assert_eq!(node[0], (prev.page_no.to_le_bytes())[0]);
        assert_eq!(node[6], (next.page_no.to_le_bytes())[0]);
    }


    fn add_last_empty() {
        let mut base = vec![0u8; FLST_LAST + FIL_ADDR_SIZE];
        let mut node = vec![0u8; FIL_ADDR_SIZE*2];
        let addr = FilAddr { page_no: 1, byte_off: 2 };
        addr.add_last(&mut base, &mut node, None);
        assert_eq!(FilAddr::len(&base), 1);
        assert_eq!(FilAddr::first(&base), addr);
        assert_eq!(FilAddr::last(&base), addr);
        assert_eq!(FilAddr::prev(&node), FilAddr::NULL);
        assert_eq!(FilAddr::next(&node), FilAddr::NULL);
    }

    #[test]
    fn add_last_two() {
        let mut base = vec![0u8; FLST_LAST + FIL_ADDR_SIZE];
        let mut n1 = vec![0u8; FIL_ADDR_SIZE*2];
        let mut n2 = vec![0u8; FIL_ADDR_SIZE*2];
        let a1 = FilAddr{ page_no: 10, byte_off: 20 };
        let a2 = FilAddr{ page_no: 30, byte_off: 40 };
        a1.add_last(&mut base, &mut n1, None);
        a2.add_last(&mut base, &mut n2, Some(&mut n1));
        assert_eq!(FilAddr::len(&base), 2);
        assert_eq!(FilAddr::prev(&n1), FilAddr::NULL);
        assert_eq!(FilAddr::next(&n1), a2);
        assert_eq!(FilAddr::prev(&n2), a1);
        assert_eq!(FilAddr::next(&n2), FilAddr::NULL);
    }

    fn remove_only_node() {
        let mut base = vec![0u8; 16*1024];
        let mut node = vec![0u8; FIL_ADDR_SIZE * 2];

        // imagine we’d already done add_last:
        // me is the addr of the node.
        let me = FilAddr { page_no: 7, byte_off: 123 };
        me.write_first(&mut base);
        me.write_last(&mut base);
        me.write_len(&mut base, 1);
        // node.prev = NULL, node.next = NULL
        FilAddr::NULL.write_prev(&mut node);
        FilAddr::NULL.write_next(&mut node);

        // Now remove it:
        me.remove(&mut base, &node, None, None);

        assert_eq!(FilAddr::len(&base), 0);
        assert_eq!(FilAddr::first(&base), FilAddr::NULL);
        assert_eq!(FilAddr::last(&base),  FilAddr::NULL);
    }

    #[test]
    fn remove_middle_node() {
        // 1) Prepare a full-page buffer and three node buffers (each 12 bytes)
        let mut base  = vec![0u8; 16*1024];
        let mut n1    = vec![0u8; FIL_ADDR_SIZE * 2];
        let mut n2    = vec![0u8; FIL_ADDR_SIZE * 2];
        let mut n3    = vec![0u8; FIL_ADDR_SIZE * 2];

        // 2) Define addresses for each node
        let addr1 = FilAddr { page_no: 1, byte_off: 10 };
        let addr2 = FilAddr { page_no: 2, byte_off: 20 };
        let addr3 = FilAddr { page_no: 3, byte_off: 30 };

        // 3) Build the list: [addr1] → [addr2] → [addr3]
        addr1.add_last(&mut base, &mut n1, None);
        addr2.add_last(&mut base, &mut n2, Some(&mut n1));
        addr3.add_last(&mut base, &mut n3, Some(&mut n2));
        // Now: len=3, first=addr1, last=addr3

        // 4) Remove the middle node (addr2)
        addr2.remove(
            &mut base,
            &n2,
            Some(&mut n1),
            Some(&mut n3),
        );

        // 5) Assertions:
        // - length decremented
        assert_eq!(FilAddr::len(&base), 2);
        // - first and last remain the same
        assert_eq!(FilAddr::first(&base), addr1);
        assert_eq!(FilAddr::last (&base), addr3);
        // - n1.next now points to addr3
        assert_eq!(FilAddr::next(&n1), addr3);
        // - n3.prev now points to addr1
        assert_eq!(FilAddr::prev(&n3), addr1);
    }




}
