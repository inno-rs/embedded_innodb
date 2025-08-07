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
use std::ops::Range;

use crate::{
    innodb::innodb0types::Frame,
    mach::mach0data::{
        mach_read_from_2, mach_read_from_4, mach_write_to_2, mach_write_to_4,
    },
    some_or_panic,
    ut::ut0dbg::ut_ad,
};

// On-disk list-node constants specific to this module
pub const FIL_ADDR_PAGE: usize = 0; // page_no at buf[offset+0..offset+4]
pub const FIL_ADDR_BYTE: usize = 4; // byte_off at buf[offset+4..offset+6]
pub const FIL_ADDR_SIZE: usize = 6; // total size for a FilAddr

pub const FLST_LEN: usize = 0; // length field at buf[0..4] of a base node
pub const FLST_FIRST: usize = 4; // first pointer at buf[4..10]
pub const FLST_LAST: usize = FLST_FIRST + FIL_ADDR_SIZE; // last pointer at buf[10..16]

const FLST_FIRST_PAGE_NO_RANGE: Range<usize> = FLST_FIRST..FLST_FIRST + 4;
const FLST_FIRST_BYTE_OFF_RANGE: Range<usize> = FLST_FIRST + 4..FLST_FIRST + 6;

const FLST_LAST_PAGE_NO_RANGE: Range<usize> = FLST_LAST..FLST_LAST + 4;
const FLST_LAST_BYTE_OFF_RANGE: Range<usize> = FLST_LAST + 4..FLST_LAST + 6;

pub const FLST_PREV: usize = 0; // prev pointer at buf[0..6] of a node
pub const FLST_NEXT: usize = FIL_ADDR_SIZE; // next pointer at buf[6..12] of a node

const FLST_PREV_PAGE_NO_RANGE: Range<usize> = FLST_PREV..FLST_PREV + 4;
const FLST_PREV_BYTE_OFF_RANGE: Range<usize> = FLST_PREV + 4..FLST_PREV + 6;

const FLST_NEXT_PAGE_NO_RANGE: Range<usize> = FLST_NEXT..FLST_NEXT + 4;
const FLST_NEXT_BYTE_OFF_RANGE: Range<usize> = FLST_NEXT + 4..FLST_NEXT + 6;

/// A 6-byte on-disk pointer: 4 bytes page number + 2 bytes byte offset.
///
/// Read more here https://docs.google.com/document/d/17Cv1R1ELip8PYuCrQFgbkxe1KRfgsjWFyCje9bS9dRI/edit?tab=t.0#heading=h.z4emgsn9c5v6
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilAddr {
    pub page_no: u32,
    pub byte_off: u16,
}

impl FilAddr {
    /// All-0xFF page_no means “null pointer”
    pub const NULL: FilAddr = FilAddr { page_no: u32::MAX, byte_off: 0 };

    /// Write this 6-byte FilAddr into `buf[offset..offset+FIL_ADDR_SIZE]`
    pub fn write(self, buf: &mut [u8], offset: usize) {
        // (Optional) sanity‐check in debug builds
        debug_assert!(
            buf.len() >= offset + FIL_ADDR_SIZE,
            "buffer too small for FilAddr::write"
        );

        // 1) write the 4-byte little-endian page_no
        buf[offset + FIL_ADDR_PAGE..offset + FIL_ADDR_PAGE + 4]
            .copy_from_slice(&self.page_no.to_le_bytes());

        // 2) write the 2-byte little-endian byte_off
        buf[offset + FIL_ADDR_BYTE..offset + FIL_ADDR_BYTE + 2]
            .copy_from_slice(&self.byte_off.to_le_bytes());
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
        base[FLST_LEN..FLST_LEN + 4].copy_from_slice(&bytes);
    }

    /// Write the 6-byte “first” pointer in a base-node
    pub fn write_first(self, base: &mut [u8]) {
        self.write(base, FLST_FIRST)
    }

    /// Write the 6-byte “last” pointer in a base-node
    pub fn write_last(self, base: &mut [u8]) {
        self.write(base, FLST_LAST)
    }

    /// Write the 6-byte “prev” pointer in a list-node
    pub fn write_prev(self, node: &mut [u8]) {
        self.write(node, FLST_PREV)
    }

    /// Write the 6-byte “next” pointer in a list-node
    pub fn write_next(self, node: &mut [u8]) {
        self.write(node, FLST_NEXT)
    }

    /// Read a FilAddr from `buf[offset..offset+6]`.
    pub fn read(buf: &[u8], offset: usize) -> FilAddr {
        let page_no = u32::from_le_bytes(
            buf[offset + FIL_ADDR_PAGE..offset + FIL_ADDR_PAGE + 4]
                .try_into()
                .unwrap(),
        );
        let byte_off = u16::from_le_bytes(
            buf[offset + FIL_ADDR_BYTE..offset + FIL_ADDR_BYTE + 2]
                .try_into()
                .unwrap(),
        );
        FilAddr { page_no, byte_off }
    }

    /// In a **base-node** (the list head), read the 4-byte length.
    pub fn len(base: &[u8]) -> usize {
        u32::from_le_bytes(base[FLST_LEN..FLST_LEN + 4].try_into().unwrap())
            as usize
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
        let len = FilAddr::len(base);
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

    /// Remove *this* node from the intrusive list in `base`.
    /// Can remove the item from anywhere in the list.
    /// - `node_buf` is the 12-byte PREV/NEXT area for this node.
    /// - `prev_node_buf` is `Some(&mut [u8])` of the previous neighbor, if any.
    /// - `next_node_buf` is `Some(&mut [u8])` of the next neighbor, if any.
    ///
    pub fn remove(
        self,
        base: &mut [u8],
        node_buf: &[u8],
        prev_node_buf: Option<&mut [u8]>,
        next_node_buf: Option<&mut [u8]>,
    ) {
        // 1) load list state
        let len = FilAddr::len(base);
        let prev_addr = FilAddr::prev(node_buf);
        let next_addr = FilAddr::next(node_buf);

        // 2) patch the previous neighbor (or FIRST in base)
        if prev_addr != FilAddr::NULL {
            let prev_buf = prev_node_buf
                .expect("prev_node_buf required when prev != NULL");
            // old-prev.next = my next
            next_addr.write_next(prev_buf);
        } else {
            // I was the first node → update base.FIRST = my next
            next_addr.write_first(base);
        }

        // 3) patch the next neighbor (or LAST in base)
        if next_addr != FilAddr::NULL {
            let next_buf = next_node_buf
                .expect("next_node_buf required when next != NULL");
            // old-next.prev = my prev
            prev_addr.write_prev(next_buf);
        } else {
            // I was the last node → update base.LAST = my prev
            prev_addr.write_last(base);
        }

        // 4) decrement the stored length
        self.write_len(base, len - 1);
    }
}

/// /// Initialize the base-node in `base` as an empty intrusive list:
// ///  - LEN = 0
// ///  - FIRST = NULL
// ///  - LAST  = NULL
pub fn flst_init(base: &mut [u8]) {
    // 1) zero length
    write_len(base, 0);
    // 2) set FIRST = NULL
    FilAddr::NULL.write_first(base);
    // 3) set LAST  = NULL
    FilAddr::NULL.write_last(base);
}

pub fn write_len(base: &mut [u8], len: usize) {
    let bytes = (len as u32).to_be_bytes();
    base[FLST_LEN..FLST_LEN + 4].copy_from_slice(&bytes);
}

const FLST_BASE_NODE_LEN: usize = 16;
/// A base-node for an intrusive list, which contains the length and pointers to the first and last nodes.
/// The base-node is used to anchor the list and provides a uniform way to access the list
/// properties such as length, first, and last nodes.
pub struct FLstBaseNode<'a> {
    /// The fil address of the node.
    fil_addr: FilAddr,
    /// The buffer of the node.
    buf: Frame<'a, FLST_BASE_NODE_LEN>,
}

impl<'a> FLstBaseNode<'a> {
    pub fn new(fil_addr: FilAddr, buf: Frame<'a, FLST_BASE_NODE_LEN>) -> Self {
        Self { fil_addr, buf }
    }

    /// Check if the list is empty.
    ///
    /// # Returns
    ///
    /// `true` if the list is empty, `false` otherwise.
    ///
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the length of the list.
    ///
    /// # Returns
    ///
    /// `usize` specifying the length of the list.
    ///
    pub fn len(&self) -> usize {
        mach_read_from_4(&self.buf.0[FLST_LEN..FLST_LEN + 4]) as usize
    }

    /// Set the length of the list.
    ///
    /// # Arguments
    ///
    /// * `len` - Length of the list to set, as u32.
    ///
    pub fn set_len(&mut self, len: u32) {
        mach_write_to_4(&mut self.buf.0[FLST_LEN..FLST_LEN + 4], len);
    }

    /// Get the fil address of the base node.
    ///
    /// # Returns
    ///
    /// `FilAddr` of the base node.
    ///
    pub fn get_fil_addr(&self) -> FilAddr {
        self.fil_addr
    }

    /// Get the first node of the list.
    ///
    /// # Returns
    ///
    /// `FilAddr` of the first node.
    ///
    pub fn get_first(&self) -> FilAddr {
        FilAddr {
            page_no: mach_read_from_4(&self.buf.0[FLST_FIRST_PAGE_NO_RANGE]),
            byte_off: mach_read_from_2(&self.buf.0[FLST_FIRST_BYTE_OFF_RANGE]),
        }
    }

    /// Set the first node of the linked list.
    ///
    /// # Arguments
    ///
    /// * `addr` - `FilAddr` of the first node.
    ///
    pub fn set_first(&mut self, addr: FilAddr) {
        // 1. write the 4-byte page_no in big-endian format.
        mach_write_to_4(
            &mut self.buf.0[FLST_FIRST_PAGE_NO_RANGE],
            addr.page_no,
        );

        // 2. write the 2-byte byte_off in big-endian format.
        mach_write_to_2(
            &mut self.buf.0[FLST_FIRST_BYTE_OFF_RANGE],
            addr.byte_off,
        )
    }

    /// Get the fil address of the last node.
    ///
    /// # Returns
    ///
    /// `FilAddr` of the last node of the list.
    ///
    pub fn get_last(&self) -> FilAddr {
        FilAddr {
            page_no: mach_read_from_4(&self.buf.0[FLST_LAST_PAGE_NO_RANGE]),
            byte_off: mach_read_from_2(&self.buf.0[FLST_LAST_BYTE_OFF_RANGE]),
        }
    }

    /// Set the last node of the linked list.
    ///
    /// # Arguments
    ///
    /// * `addr` - `FilAddr` of the last node.
    ///
    pub fn set_last(&mut self, addr: FilAddr) {
        // 1. write the 4-byte page_no in big-endian format.
        mach_write_to_4(
            &mut self.buf.0[FLST_LAST_PAGE_NO_RANGE],
            addr.page_no,
        );

        // 2. write the 2-byte byte_off in big-endian format.
        mach_write_to_2(
            &mut self.buf.0[FLST_LAST_BYTE_OFF_RANGE],
            addr.byte_off,
        )
    }
}

const FLST_NODE_LEN: usize = 12;
pub struct FLstNode<'a> {
    fil_addr: FilAddr,
    buf: Frame<'a, FLST_NODE_LEN>,
}

impl<'a> FLstNode<'a> {
    pub fn new(fil_addr: FilAddr, buf: Frame<'a, FLST_NODE_LEN>) -> Self {
        Self { fil_addr, buf }
    }

    pub fn get_fil_addr(&self) -> FilAddr {
        self.fil_addr
    }

    pub fn get_prev(&self) -> FilAddr {
        FilAddr {
            page_no: mach_read_from_4(&self.buf.0[FLST_PREV_PAGE_NO_RANGE]),
            byte_off: mach_read_from_2(&self.buf.0[FLST_PREV_BYTE_OFF_RANGE]),
        }
    }

    pub fn set_prev(&mut self, addr: FilAddr) {
        // 1) write the 4-byte page_no in big-endian format.
        mach_write_to_4(
            &mut self.buf.0[FLST_PREV_PAGE_NO_RANGE],
            addr.page_no,
        );

        // 2) write the 2-byte byte_off in big-endian format.
        mach_write_to_2(
            &mut self.buf.0[FLST_PREV_BYTE_OFF_RANGE],
            addr.byte_off,
        );
    }

    pub fn get_next(&self) -> FilAddr {
        FilAddr {
            page_no: mach_read_from_4(&self.buf.0[FLST_NEXT_PAGE_NO_RANGE]),
            byte_off: mach_read_from_2(&self.buf.0[FLST_NEXT_BYTE_OFF_RANGE]),
        }
    }

    pub fn set_next(&mut self, addr: FilAddr) {
        // 1) write the 4-byte page_no in big-endian format.
        mach_write_to_4(
            &mut self.buf.0[FLST_NEXT_PAGE_NO_RANGE],
            addr.page_no,
        );

        // 2) write the 2-byte byte_off in big-endian format.
        mach_write_to_2(
            &mut self.buf.0[FLST_NEXT_BYTE_OFF_RANGE],
            addr.byte_off,
        );
    }
}

/// A file-based doubly-linked list (intrusive list) implementation.
/// This structure provides methods to manipulate the list, such as adding nodes to the end,
/// removing nodes, and accessing the first and last nodes.
/// It uses `FLstBaseNode` to anchor the list and `FLstNode` to represent each node in the list.
pub struct FLst<'a> {
    base: FLstBaseNode<'a>,
}

impl<'a> FLst<'a> {
    pub fn new(base: FLstBaseNode<'a>) -> Self {
        Self { base }
    }

    /// Add a node at the end of the linked list.
    ///
    /// # Arguments
    ///
    /// * `node` - A mutable reference to the node to be added.
    /// * `last` - An optional mutable reference to the last node in the list.
    ///
    pub fn add_last(
        &mut self,
        node: &mut FLstNode<'a>,
        last: Option<FLstNode<'a>>,
    ) {
        // Get the current length of the list
        let len = self.base.len();

        // If the list is empty, set the first and last pointers to the new node
        if len == 0 {
            self.base.set_first(node.get_fil_addr());
            self.base.set_last(node.get_fil_addr());
        } else {
            // If the list is not empty, the caller should provide the previous last node.
            let mut prev = some_or_panic!(last);
            // Otherwise, we need to update the last node's next pointer
            let last_addr = self.base.get_last();
            ut_ad!(last_addr != prev.get_fil_addr());

            prev.set_next(node.get_fil_addr());
            node.set_next(FilAddr::NULL);
            node.set_prev(last_addr);

            self.base.set_last(node.get_fil_addr());
        }

        // Increment the length of the list
        self.base.set_len((len + 1) as u32);
    }

    /// Remove *this* node from the intrusive list in `base`.
    /// Can remove the item from anywhere in the list.
    /// - `node_buf` is the 12-byte PREV/NEXT area for this node.
    /// - `prev_node_buf` is `Some(&mut [u8])` of the previous neighbor, if any.
    /// - `next_node_buf` is `Some(&mut [u8])` of the next neighbor, if any.
    ///
    /// # Arguments
    ///
    /// * `node` - The node to remove to.
    /// * `prev_node` - An optional previous node in the list.
    /// * `next_node` - An optional next node in the list.
    ///
    pub fn remove(
        mut self,
        node: FLstNode,
        prev_node: Option<FLstNode>,
        next_node: Option<FLstNode>,
    ) {
        // 1) load list state
        let len = self.base.len();

        // 2) patch the previous neighbor (or FIRST in base)
        if let Some(mut prev_node) = prev_node {
            ut_ad!(
                prev_node.get_fil_addr() != node.get_prev(),
                "prev_node_buf required when prev != NULL"
            );

            // old-prev.next = my next
            prev_node.set_next(node.get_next());
        } else {
            // I was the first node → update base.FIRST to my next
            self.base.set_first(node.get_next());
        }

        // 3) patch the next neighbor (or LAST in base)
        if let Some(mut next_node) = next_node {
            ut_ad!(
                next_node.get_fil_addr() != node.get_next(),
                "next_node_buf required when next != NULL"
            );

            // old-next.prev = my prev
            next_node.set_prev(node.get_prev());
        } else {
            // I was the last node → update base.LAST = my prev
            self.base.set_last(node.get_prev());
        }

        // 4) decrement the stored length
        self.base.set_len((len - 1) as u32);
    }

    /// Get the last element of the linked list.
    ///
    /// # Returns
    ///
    /// `FillAddr` of the last element in the list.
    ///
    pub fn get_last(&self) -> FilAddr {
        self.base.get_last()
    }

    /// Get the first element of the linked list.
    ///
    /// # Returns
    ///
    /// `FillAddr` of the first element in the list.
    ///
    pub fn get_first(&self) -> FilAddr {
        self.base.get_first()
    }

    /// Get the length of the list
    ///
    /// # Returns
    ///
    /// `usize` representing the length of the list.
    ///
    pub fn len(&self) -> usize {
        self.base.len()
    }

    /// Check if the list is empty
    ///
    /// # Returns
    ///
    /// `true` is the list is empty, `false` otherwise.
    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
    }

    // ****** Start Low-level list operations ******
    // The methods listed below are only used in the scenarios where the caller takes on the responsibility of managing the list state.

    /// Set the first element of the linked list.
    ///
    /// * `addr` - `FilAddr` of the first element to be added.
    ///
    pub fn set_first(&mut self, addr: FilAddr) {
        self.base.set_first(addr);
    }

    /// Set the first element of the linked list.
    ///
    /// * `addr` - `FilAddr` of the last element to be added.
    ///
    pub fn set_last(&mut self, addr: FilAddr) {
        self.base.set_last(addr);
    }

    /// Increase the length of the linked list by 1.
    pub fn incr_length(&mut self) {
        self.base.set_len((self.base.len() + 1) as u32);
    }

    /// Decrease the length of the linked list by 1.
    pub fn decr_length(&mut self) {
        self.base.set_len((self.base.len() - 1) as u32);
    }

    // ****** End Low-level list operations ******
}
