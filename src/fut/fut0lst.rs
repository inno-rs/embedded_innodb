

/*
The fut0lst module (in C++ fut0lst.h, here ported to fut0lst.rs) is simply InnoDB’s on-disk intrusive doubly-linked-list library. Its job is to give you a uniform, low-level way to:

Anchor a list in a header (the “base-node”), which stores:

A 4-byte length (LEN)

A 6-byte pointer to the first element (FIRST)

A 6-byte pointer to the last element (LAST)

Link each element (a “list-node” embedded in, say, an extent descriptor) by storing in that element:

A 6-byte prev pointer (PREV)

A 6-byte next pointer (NEXT)

All pointers on-disk are represented by a 6-byte FilAddr (4 bytes of page number + 2 bytes of byte-offset within that page).

With these primitives you can in O(1):

Append a new element to the tail of any list (flst_add_last),

Remove an existing element from anywhere in the list (flst_remove),

Traverse forward or backward through the chain.



*/
// On-disk list-node constants specific to this module
pub const FIL_ADDR_PAGE: usize = 0;                    // page_no at buf[offset+0..offset+4]
pub const FIL_ADDR_BYTE: usize = 4;                    // byte_off at buf[offset+4..offset+6]
pub const FIL_ADDR_SIZE: usize = 6;                // total size for a FilAddr

pub const FLST_LEN: usize = 0;                         // length field at buf[0..4] of a base node
pub const FLST_FIRST: usize = 4;                       // first pointer at buf[4..10]
pub const FLST_LAST: usize = FLST_FIRST + FIL_ADDR_SIZE;  // last pointer at buf[10..16]

pub const FLST_PREV: usize = 0;                        // prev pointer at buf[0..6] of a node
pub const FLST_NEXT: usize = FIL_ADDR_SIZE;            // next pointer at buf[6..12] of a node

/// A 6-byte on-disk pointer: 4 bytes page number + 2 bytes byte offset.
///
/// Read more here https://docs.google.com/document/d/17Cv1R1ELip8PYuCrQFgbkxe1KRfgsjWFyCje9bS9dRI/edit?tab=t.0#heading=h.z4emgsn9c5v6
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilAddr {
    pub page_no:  u32,
    pub byte_off: u16,
}

impl FilAddr {
    /// All-0xFF page_no means “null pointer”
    pub const NULL: FilAddr = FilAddr {
        page_no: u32::MAX,
        byte_off: 0,
    };


    /// Write this 6-byte FilAddr into `buf[offset..offset+FIL_ADDR_SIZE]`
    pub fn write(self, buf: &mut [u8], offset: usize) {
        // (Optional) sanity‐check in debug builds
        debug_assert!(
            buf.len() >= offset + FIL_ADDR_SIZE,
            "buffer too small for FilAddr::write"
        );

        // 1) write the 4-byte little-endian page_no
        buf[offset + FIL_ADDR_PAGE .. offset + FIL_ADDR_PAGE + 4].copy_from_slice(&self.page_no.to_le_bytes());

        // 2) write the 2-byte little-endian byte_off
        buf[offset + FIL_ADDR_BYTE .. offset + FIL_ADDR_BYTE + 2].copy_from_slice(&self.byte_off.to_le_bytes());
    }

    /// Write the 4-byte length in a base-node. Below is the base node diagram
    /// First and Last are the pointers to first list node and  last list node
    /// ┌─────────────────────────────────────┐
    // │ bytes 0..4 │ bytes 4..10 │ bytes 10..16 │
    // │   LEN      │   FIRST     │    LAST      │
    // └─────────────────────────────────────┘
    /// Write the 4‐byte list‐length (LE u32) into a base‐node at offset FLST_LEN.
    pub fn write_len(self, base: &mut [u8], len: usize) {
        // In debug builds, ensure we have room for 4 bytes at FLST_LEN
        debug_assert!(
            base.len() >= FLST_LEN + 4,
            "buffer too small for write_len"
        );
        // Convert the usize (we assume it fits in u32) into 4 little‐endian bytes
        let bytes = (len as u32).to_le_bytes();
        // Copy those bytes into base[FLST_LEN..FLST_LEN+4]
        base[FLST_LEN .. FLST_LEN + 4].copy_from_slice(&bytes);
    }


    /// Write the 6-byte “first” pointer in a base-node
    pub fn write_first(self, base: &mut [u8]) { self.write(base, FLST_FIRST) }

    /// Write the 6-byte “last” pointer in a base-node
    pub fn write_last(self, base: &mut [u8]) { self.write(base, FLST_LAST) }

    /// Write the 6-byte “prev” pointer in a list-node
    pub fn write_prev(self, node: &mut [u8]) { self.write(node, FLST_PREV) }

    /// Write the 6-byte “next” pointer in a list-node
    pub fn write_next(self, node: &mut [u8]) { self.write(node, FLST_NEXT) }



    /// Read a FilAddr from `buf[offset..offset+6]`.
    pub fn read(buf: &[u8], offset: usize) -> FilAddr {
        let page_no = u32::from_le_bytes(
            buf[offset + FIL_ADDR_PAGE .. offset + FIL_ADDR_PAGE + 4]
                .try_into()
                .unwrap(),
        );
        let byte_off = u16::from_le_bytes(
            buf[offset + FIL_ADDR_BYTE .. offset + FIL_ADDR_BYTE + 2]
                .try_into()
                .unwrap(),
        );
        FilAddr { page_no, byte_off }
    }

    /// In a **base-node** (the list head), read the 4-byte length.
    pub fn len(base: &[u8]) -> usize {
        u32::from_le_bytes(
            base[FLST_LEN .. FLST_LEN + 4]
                .try_into()
                .unwrap(),
        ) as usize
    }

    /// In a **base-node**, read the 6-byte “first” pointer.
    pub fn first(base: &[u8]) -> FilAddr {
        FilAddr::read(base, FLST_FIRST)
    }

    /// In a **base-node**, read the 6-byte “last” pointer.
    pub fn last(base: &[u8]) -> FilAddr {
        FilAddr::read(base, FLST_LAST)
    }

    /// In a **list-node**, read the 6-byte “prev” pointer.
    pub fn prev(node: &[u8]) -> FilAddr {
        FilAddr::read(node, FLST_PREV)
    }

    /// In a **list-node**, read the 6-byte “next” pointer.
    pub fn next(node: &[u8]) -> FilAddr {
        FilAddr::read(node, FLST_NEXT)
    }


    /// Append *this* node to the tail of the list in `base`.
    ///
    /// - `new_node_buf` must be the 12-byte PREV/NEXT area for this node
    /// - `prev_node_buf` is `Some(&mut [u8])` of the old last node, or `None` if empty
    pub fn add_last(
        self,
        base: &mut [u8],
        new_node_buf: &mut [u8],
        prev_node_buf: Option<&mut [u8]>,
    ) {
        // 1) list state
        let len       = FilAddr::len(base);
        let last_addr = FilAddr::last(base);

        // 1) init new node's back-pointer
        if len == 0 {
            // empty list → no prev
            FilAddr::NULL.write_prev(new_node_buf);
        } else {
            // non-empty → prev = old tail
            let last_addr = FilAddr::last(base);
            last_addr.write_prev(new_node_buf);
        }
        FilAddr::NULL.write_next(new_node_buf);

        // 3) patch base or old-last
        if len == 0 {
            // empty list → update FIRST
            self.write_first(base);
        } else {
            // nonempty → patch old-last.next = me
            let old_last_buf = prev_node_buf
                .expect("prev_node_buf required for nonempty list");
            // Update the next pointer of the old_last_buf
            self.write_next(old_last_buf);
        }

        // 4) set LAST and bump LEN
        self.write_last(base);
        self.write_len(base, len + 1);
    }
}
