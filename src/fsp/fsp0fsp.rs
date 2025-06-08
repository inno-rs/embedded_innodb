//! On-disk layout constants for InnoDB FSP
//!
//! ┌───────────────────────────────────────────────────────────────────────────────┐
// │                              InnoDB 16 KB Page                               │
// │                               (16 384 bytes)                                 │
// ├───────────────┬──────────────────────────────┬───────────────────────────────┤
// │ Off 0..37      │ Off 38..149                   │ Off 150..16383                │
// │ 38 bytes       │ 112 bytes                     │ 16 234 bytes (rest of page)   │
// │               ┌──────────────────────────────┐                               │
// │               │      FSP Header Region      │   Page payload, B‐trees, etc.│
// │               │ ┌───────────┬───────────┬───┐ │                               │
// │               │ │ space_id │ space_sz │…  │ │                               │
// │               │ ├───────────┼───────────┼───┤ │                               │
// │               │ │ LIST #1   │ LIST #2   │…  │ │                               │
// │               │ │ (16 B each)            …  │ │                               │
// │               │ │            NEXT_SEG_ID    │ │                               │
// │               │ └────────────────────────────┘ │                               │
// ├───────────────┴────────────────────────────────┴───────────────────────────────┤
// │ Off 150..189     Off 190..229     Off 230..269     …    Off 150+40N..189+40N  │
// │ Extent Desc 0    Extent Desc 1    Extent Desc 2    …    Extent Desc N         │
// │ (40 bytes each, packed back-to-back)                                             │
// └───────────────────────────────────────────────────────────────────────────────┘
//!Page-offset	Length	Field	Description
// 0..37	38 B	Generic page header	InnoDB’s universal per-page fields (LSN, page number, etc.)
// 38..41	4 B	space_id	Tablespace identifier (LE u32)
// 42..45	4 B	not_used	Reserved (zero)
// 46..49	4 B	space_size	Total number of pages in this space (LE u32)
// 50..53	4 B	free_limit	Maximum pages to allocate (initially = space_size)
// 54..57	4 B	space_flags	Flags (zero on init)
// 58..61	4 B	frag_n_used	Fragment-allocation counter (zero on init)
// 62..77	16 B	FREE list head	(len:4, first:6, last:6) for completely free extents
// 78..93	16 B	FREE_FRAG list head	for partially free extents
// 94..109	16 B	FULL_FRAG list head	for fully used extents
// 110..113	4 B	next_segment_id	Counter for new segments (zero on init)
// 114..117	4 B	(padding/reserved)	Unused on init
// 118..133	16 B	SEG_INODES_FULL list head	for “full” segment‐inode pages
// 134..149	16 B	SEG_INODES_FREE list head	for “free” segment‐inode pages
// 150..16383	16 368 B	Page body	Remainder of the 16 KB page (payload, free space, etc.)
//!
//!

use crate::fil::fil0types::{
    FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN, FIL_PAGE_OFFSET, FIL_PAGE_SPACE_ID,
};
use crate::fut::fut0lst::{flst_init, FilAddr};
use crate::innodb::innodb0types::{SpaceId, UNIV_PAGE_SIZE};
use std::ops::Range;

/// Offset of the page data area inside a raw page
pub const FIL_PAGE_DATA: usize = 38;

/// Offset of the segment-inode page data inside a raw page
pub const FSEG_PAGE_DATA: usize = FIL_PAGE_DATA;

/// Size of an address in FIL_ADDR (bytes)
pub const FIL_ADDR_SIZE: usize = 6;

/// Base size of a list node (bytes) from fut0lst.h: 4 + 2 * FIL_ADDR_SIZE
pub const FLST_BASE_NODE_SIZE: usize = 4 + 2 * FIL_ADDR_SIZE;

/// Size of the list node linking multiple pages (bytes)
pub const FLST_NODE_SIZE: usize = 2 * FIL_ADDR_SIZE;

/// Number of pages per extent in InnoDB
pub const FSP_EXTENT_SIZE: usize = 64;

/// Compute how many bytes are needed to store the given number of bits
#[inline]
pub const fn ut_bits_in_bytes(bits: usize) -> usize {
    (bits + 7) / 8
}

// Space header offsets and sizes
pub const FSP_HEADER_OFFSET: usize = FIL_PAGE_DATA;
pub const FSP_SPACE_ID: usize = 0;
pub const FSP_NOT_USED: usize = 4;
pub const FSP_SIZE: usize = 8;
pub const FSP_FREE_LIMIT: usize = 12;
pub const FSP_SPACE_FLAGS: usize = 16;
pub const FSP_FRAG_N_USED: usize = 20;
pub const FSP_FREE: usize = 24;
pub const FSP_FREE_FRAG: usize = 24 + FLST_BASE_NODE_SIZE;
pub const FSP_FULL_FRAG: usize = 24 + 2 * FLST_BASE_NODE_SIZE;
pub const FSP_SEG_ID: usize = 24 + 3 * FLST_BASE_NODE_SIZE;
pub const FSP_SEG_INODES_FULL: usize = 32 + 3 * FLST_BASE_NODE_SIZE;
pub const FSP_SEG_INODES_FREE: usize = 32 + 4 * FLST_BASE_NODE_SIZE;
pub const FSP_HEADER_SIZE: usize = 32 + 5 * FLST_BASE_NODE_SIZE;
pub const FSP_FREE_ADD: usize = 4;

// Segment inode page constants
pub const FSEG_INODE_PAGE_NODE: usize = FSEG_PAGE_DATA;
pub const FSEG_ARR_OFFSET: usize = FSEG_PAGE_DATA + FLST_NODE_SIZE;

// Segment header offsets
pub const FSEG_ID: usize = 0;
pub const FSEG_NOT_FULL_N_USED: usize = 8;
pub const FSEG_FREE: usize = 12;
pub const FSEG_NOT_FULL: usize = 12 + FLST_BASE_NODE_SIZE;
pub const FSEG_FULL: usize = 12 + 2 * FLST_BASE_NODE_SIZE;
pub const FSEG_MAGIC_N: usize = 12 + 3 * FLST_BASE_NODE_SIZE;
pub const FSEG_FRAG_ARR: usize = 16 + 3 * FLST_BASE_NODE_SIZE;
pub const FSEG_FRAG_ARR_N_SLOTS: usize = FSP_EXTENT_SIZE / 2;
pub const FSEG_FRAG_SLOT_SIZE: usize = 4;

// Inode size and per-page count
pub const FSEG_INODE_SIZE: usize =
    16 + 3 * FLST_BASE_NODE_SIZE + FSEG_FRAG_ARR_N_SLOTS * FSEG_FRAG_SLOT_SIZE;
pub const FSP_SEG_INODES_PER_PAGE: usize =
    (UNIV_PAGE_SIZE - FSEG_ARR_OFFSET - 10) / FSEG_INODE_SIZE;
pub const FSEG_MAGIC_N_VALUE: usize = 97937874;

// Fillfactor and fragment limits
pub const FSEG_FILLFACTOR: usize = 8;
pub const FSEG_FRAG_LIMIT: usize = FSEG_FRAG_ARR_N_SLOTS;
pub const FSEG_FREE_LIST_LIMIT: usize = 40;
pub const FSEG_FREE_LIST_MAX_LEN: usize = 4;

// Extent descriptor constants
pub const XDES_ID: usize = 0;
pub const XDES_FLST_NODE: usize = 8;
pub const XDES_STATE: usize = FLST_NODE_SIZE + 8;
pub const XDES_BITMAP: usize = FLST_NODE_SIZE + 12;
pub const XDES_BITS_PER_PAGE: usize = 2;
pub const XDES_FREE_BIT: usize = 0;
pub const XDES_CLEAN_BIT: usize = 1;
pub const XDES_FREE: usize = 1;
pub const XDES_FREE_FRAG: usize = 2;
pub const XDES_FULL_FRAG: usize = 3;
pub const XDES_FSEG: usize = 4;
pub const XDES_SIZE: usize =
    XDES_BITMAP + ut_bits_in_bytes(FSP_EXTENT_SIZE * XDES_BITS_PER_PAGE);
pub const XDES_ARR_OFFSET: usize = FSP_HEADER_OFFSET + FSP_HEADER_SIZE;

const FSP_SPACE_ID_RANGE: Range<usize> =
    FSP_HEADER_OFFSET + FSP_SPACE_ID..FSP_HEADER_OFFSET + FSP_SPACE_ID + 4;
const FSP_SPACE_FLAGS_RANGE: Range<usize> = FSP_HEADER_OFFSET + FSP_SPACE_FLAGS
    ..FSP_HEADER_OFFSET + FSP_SPACE_FLAGS + 4;

/// Represents a file space manager for InnoDB tablespaces.
///
/// The `Fsp` struct provides an interface for managing tablespace files,
/// including operations for allocating and deallocating pages and extents.
pub struct Fsp {}

impl Fsp {
    /// Creates a new file space manager instance.
    ///
    /// Returns an empty `Fsp` struct that can be used to manage tablespace operations.
    pub fn new() -> Fsp {
        Fsp {}
    }

    /// Initializes the basic fields in a file space header.
    ///
    /// This method sets up the essential fields in the FSP header of a tablespace:
    /// - The space ID that uniquely identifies this tablespace
    /// - The flags that define tablespace properties and format
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable byte slice containing the page buffer
    /// * `space_id` - The unique identifier for this tablespace
    /// * `flags` - The flags defining tablespace properties
    ///
    pub fn init_fields(buf: &mut [u8], space_id: SpaceId, flags: usize) {
        // Initialize the fields of the FSP header to zero
        buf[FSP_SPACE_ID_RANGE].copy_from_slice(&space_id.to_le_bytes());
        buf[FSP_SPACE_FLAGS_RANGE]
            .copy_from_slice(&(flags as u32).to_le_bytes());
    }
}

/// Represents a full 16 KiB InnoDB page buffer in memory.
///
/// The `PageFrame` struct provides a safe interface for manipulating the raw bytes
/// of an InnoDB page, particularly for file space management operations. It is typically
/// used to represent page 0 of a tablespace, which contains the file space header and
/// extent descriptors.
///
/// The struct wraps a mutable byte slice and provides methods to read and write
/// various fields in the page according to InnoDB's on-disk format.
pub struct PageFrame<'a> {
    /// The raw byte buffer representing the page
    buf: &'a mut [u8],
}

#[cfg(test)]
impl<'a> PageFrame<'a> {
    pub fn get_buf_mut(&mut self) -> &mut [u8] {
        &mut self.buf
    }
}

impl<'a> PageFrame<'a> {
    /// Creates a new `PageFrame` by wrapping a raw 16 KiB buffer.
    ///
    /// This method takes ownership of a mutable byte slice and provides
    /// a structured interface for manipulating it as an InnoDB page.
    ///
    /// # Arguments
    /// * `buf` - A mutable byte slice of at least UNIV_PAGE_SIZE (16 KiB) in length
    ///
    /// # Panic
    /// Panics if the provided buffer is smaller than UNIV_PAGE_SIZE
    pub fn new(buf: &'a mut [u8]) -> Self {
        assert!(
            buf.len() >= UNIV_PAGE_SIZE,
            "PageFrame needs a full UNIV_PAGE_SIZE buffer"
        );
        PageFrame { buf }
    }

    /// Initializes the FSP (File Space) header in this page.
    ///
    /// This method sets up the file space header at the beginning of the page,
    /// which includes space ID, size information, and initializes the linked lists
    /// that track free, partially free, and full extents.
    ///
    /// # Arguments
    /// * `space_id` - The unique identifier for this tablespace
    /// * `space_size` - The initial size of the tablespace in pages
    pub fn header_init(&mut self, space_id: u32, space_size: u32) {
        let start = FSP_HEADER_OFFSET; // Byte 38
        let end = start + FSP_HEADER_SIZE; // Byte 150 (Total 112 Bytes)
        self.buf[start..end].fill(0);
        self.buf[start + FSP_SPACE_ID..start + FSP_SPACE_ID + 4] // Byte 38 to Byte 41
            .copy_from_slice(&space_id.to_le_bytes());
        self.buf[start + FSP_SIZE..start + FSP_SIZE + 4] // Byte 46 to 49
            .copy_from_slice(&space_size.to_le_bytes());
        self.buf[start + FSP_FREE_LIMIT..start + FSP_FREE_LIMIT + 4] // Byte 50 to 53
            .copy_from_slice(&space_size.to_le_bytes());

        // Flags and frag_n_used are already zeroed

        // Below are 5 linked list
        let heads = [
            FSP_FREE,            // free list Byte 62-77 [38 + 24 -- 38+24+16]
            FSP_FREE_FRAG,       // partially free extents // Byte 78-93
            FSP_FULL_FRAG,       // list of full extents // Byte 94-109
            FSP_SEG_INODES_FULL, // or “full” segment‐inode pages
            FSP_SEG_INODES_FREE, // for “free” segment‐inode pages
        ];
        for &off in &heads {
            let abs = start + off;
            flst_init(&mut self.buf[abs..abs + FLST_BASE_NODE_SIZE]);
        }

        self.buf[start + FSP_SEG_ID..start + FSP_SEG_ID + 4]
            .copy_from_slice(&0u32.to_le_bytes());
    }

    /// Allocates a free extent from the FSP_FREE list, marks it as partially used,
    /// and moves it to the FSP_FREE_FRAG list.
    ///
    /// This function will:
    /// •	Pick one free extent from the free list
    /// •	Mark it as “partially used”
    /// •	Move it to the “fragmented list”
    /// •	Return its index
    pub fn alloc_free_extent(&mut self) -> Option<u32> {
        let buf = &mut self.buf;

        // Step 1: Read the first node in the FSP_FREE list (scope-limited to avoid borrow conflict)
        let (first, extent_off) = {
            let free_head = &mut buf[FSP_HEADER_OFFSET + FSP_FREE..];
            let first = FilAddr::first(free_head);
            if first == FilAddr::NULL {
                return None;
            }
            (first, first.byte_off as usize)
        };

        // Step 2: Split buffer into FSP header + extent descriptor area
        let (header_part, xdes_area) = buf.split_at_mut(XDES_ARR_OFFSET);

        // Step 3: Get mutable slice for the 40-byte extent descriptor
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];

        // Step 4: Initialize extent descriptor and mark as partially used
        let mut xdes = ExtentDescriptor::new(xdes_buf);
        xdes.xdes_init();
        xdes.set_state(XDES_FREE_FRAG);

        // Step 5: Remove from FSP_FREE list (use header_part safely now)
        {
            let free_head = &mut header_part[FSP_HEADER_OFFSET + FSP_FREE..];
            first.remove(free_head, xdes_buf, None, None);
        }

        // Step 6: Add to FSP_FREE_FRAG list
        {
            let free_frag_head =
                &mut header_part[FSP_HEADER_OFFSET + FSP_FREE_FRAG..];
            first.add_last(free_frag_head, xdes_buf, None);
        }

        // Step 7: Return extent index
        let index = (extent_off - XDES_ARR_OFFSET) / XDES_SIZE;
        Some(index as u32)
    }

    /// Frees a single page and moves the extent between lists based on usage.
    ///
    /// This method marks a page as free and updates the extent's state. If all pages
    /// in the extent become free, the extent is moved from the FREE_FRAG list to the
    /// FREE list.
    ///
    /// # Arguments
    /// * `page_no` - The page number to free
    pub fn free_page(&mut self, page_no: u32) {
        let buf = &mut self.buf;

        let extent_no = page_no / FSP_EXTENT_SIZE as u32;
        let page_idx = (page_no % FSP_EXTENT_SIZE as u32) as usize;
        let extent_off = XDES_ARR_OFFSET + extent_no as usize * XDES_SIZE;

        let (head_part, xdes_area) = buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let mut xdes = ExtentDescriptor::new(xdes_buf);

        xdes.set_bit(page_idx, XDES_FREE_BIT, true);

        if xdes.used_count() == 0 {
            xdes.set_state(XDES_FREE);

            let this_addr = FilAddr {
                page_no: extent_no * FSP_EXTENT_SIZE as u32,
                byte_off: extent_off as u16,
            };

            {
                let free_frag_head =
                    &mut head_part[FSP_HEADER_OFFSET + FSP_FREE_FRAG..];
                this_addr.remove(free_frag_head, xdes_buf, None, None);
            }

            {
                let free_head = &mut head_part[FSP_HEADER_OFFSET + FSP_FREE..];
                this_addr.add_last(free_head, xdes_buf, None);
            }
        } else {
            xdes.set_state(XDES_FREE_FRAG);
        }
    }

    /// Allocates one page from the tablespace.
    ///
    /// This method is responsible for allocating one page from the tablespace. It works by:
    /// 1. Searching partially used extents (i.e., FSP_FREE_FRAG list)
    /// 2. Picking the first extent descriptor from that list
    /// 3. Asking that extent for a free page via `ExtentDescriptor::alloc_page()`
    /// 4. Updating the list pointers and state if the extent becomes full
    ///
    /// # Returns
    /// * `Some(page_no)` - The page number of the allocated page
    /// * `None` - If no free page could be allocated
    pub fn alloc_page(&mut self) -> Option<u32> {
        // Get the FSP FREE_FRAG_LIST
        let head = &mut self.buf[FSP_HEADER_OFFSET + FSP_FREE_FRAG..];
        let first = FilAddr::first(head);
        if first == FilAddr::NULL {
            return None;
        }
        let extent_off = first.byte_off as usize;
        let (header_part, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];

        let mut xdes = ExtentDescriptor::new(xdes_buf);

        let extent_no = rel_off / XDES_SIZE;
        let page_no = xdes.alloc_page(extent_no as u32)?;

        if xdes.is_full() {
            // If extent is now full, move it to FSP_FULL_FRAG
            let this_addr =
                FilAddr { page_no: 0, byte_off: extent_off as u16 };

            // remove from FSP_FREE_FRAG
            this_addr.remove(
                &mut header_part[FSP_HEADER_OFFSET + FSP_FREE_FRAG..],
                xdes_buf,
                None,
                None,
            );

            // add to FSP_FULL_FRAG
            this_addr.add_last(
                &mut header_part[FSP_HEADER_OFFSET + FSP_FULL_FRAG..],
                xdes_buf,
                None,
            );
        }
        Some(page_no)
    }

    /*
    IMP
    Allocate one page from the tablespace, using this order of preference:
    1.	Try to find a partially-used extent (a FREE_FRAG extent) and allocate a page from it.
    2.	If no such extent exists, allocate a brand new extent (FREE extent),
    convert it into a FREE_FRAG, and allocate the first page from it.

    */
    pub fn alloc_single_page(&mut self) -> Option<u32> {
        // Step 1: Try to allocate from existing partially used extents
        if let Some(page_no) = self.alloc_page() {
            return Some(page_no);
        }

        // Step 2: No FREE_FRAG extents — try allocating a fresh extent
        let extent_no = self.alloc_free_extent()?; // None means space exhausted
        let extent_off = XDES_ARR_OFFSET + extent_no as usize * XDES_SIZE;

        // Step 3: Get mutable view into descriptor
        let (_, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let mut xdes = ExtentDescriptor::new(xdes_buf);

        // Step 4: Allocate one page from the fresh extent
        xdes.alloc_page(extent_no)
    }

    /*
    This method tries to allocate n pages from a tablespace. But InnoDB (and our simplified model) only supports:
     •	allocating 1 page (from a partially free extent),
     •	or a full extent of 64 pages (all pages from a fresh extent).

    */
    pub fn alloc_n_pages(&mut self, n: usize) -> Option<Vec<u32>> {
        if n == 0 {
            return None;
        }

        if n == 1 {
            // “If alloc_single_page() gives me Some(p),
            // wrap that p in a new vector vec![p]. p = pageNumber. Otherwise, propagate None.”
            return self.alloc_single_page().map(|p| vec![p]);
        }

        if n == FSP_EXTENT_SIZE {
            let extent_no = self.alloc_free_extent()?;
            let base_page = extent_no * (FSP_EXTENT_SIZE as u32);
            let pages =
                (0..FSP_EXTENT_SIZE).map(|i| base_page + i as u32).collect();
            return Some(pages); // Returning page numbers
        }

        // InnoDB doesn't support allocating partial extents
        None
    }

    /// Extends the space by one extent and returns the new extent number
    pub fn extend_space(&mut self) -> Option<u32> {
        // 1. Read current space size
        let current_pages = self.read_u32_le(FSP_HEADER_OFFSET + FSP_SIZE); // Read 4 bytes - Byte 46-49

        // 2. Determine new extent number and its byte offset
        let new_extent_no = current_pages / FSP_EXTENT_SIZE as u32;
        let extent_off = XDES_ARR_OFFSET + new_extent_no as usize * XDES_SIZE;
        let rel_off = extent_off - XDES_ARR_OFFSET;

        // 3. SAFELY split buffer once into header and xdes area
        let (header_part, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);

        // 4. Get a mutable slice of the new extent descriptor
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let mut xdes = ExtentDescriptor::new(xdes_buf);
        xdes.xdes_init(); // All pages free
        xdes.set_state(XDES_FREE);

        // 5. Add this extent to the FSP_FREE list
        let free_head = &mut header_part[FSP_HEADER_OFFSET + FSP_FREE
            ..FSP_HEADER_OFFSET + FSP_FREE + FLST_BASE_NODE_SIZE];
        let this_addr = FilAddr {
            page_no: 0, // Can be 0 because we're not using it here
            byte_off: extent_off as u16,
        };
        this_addr.add_last(free_head, xdes_buf, None);

        // 6. Update space size
        let new_size = current_pages + FSP_EXTENT_SIZE as u32;
        self.write_u32_le(FSP_HEADER_OFFSET + FSP_SIZE, new_size);

        Some(new_extent_no)
    }

    pub fn read_u32_le(&self, offset: usize) -> u32 {
        let bytes = &self.buf[offset..offset + 4];
        u32::from_le_bytes(bytes.try_into().expect("Failed to read u32"))
    }

    pub fn write_u32_le(&mut self, offset: usize, value: u32) {
        let bytes = value.to_le_bytes();
        self.buf[offset..offset + 4].copy_from_slice(&bytes);
    }

    pub fn is_page_free(&mut self, page_no: u32) -> bool {
        let extent_no = page_no / FSP_EXTENT_SIZE as u32;
        let page_idx = (page_no % FSP_EXTENT_SIZE as u32) as usize;
        let extent_off = XDES_ARR_OFFSET + extent_no as usize * XDES_SIZE;

        // SAFELY access the descriptor
        let (_, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let xdes = ExtentDescriptor::new(xdes_buf);

        xdes.get_bit(page_idx, XDES_FREE_BIT)
    }
    pub fn is_extent_used(&mut self, extent_no: u32) -> bool {
        let extent_off = XDES_ARR_OFFSET + extent_no as usize * XDES_SIZE;

        let (_, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        let xdes = ExtentDescriptor::new(xdes_buf);

        xdes.used_count() > 0
    }

    pub fn space_available(&mut self) -> usize {
        let mut free_pages = 0;
        let total_extents = (self.read_u32_le(FSP_HEADER_OFFSET + FSP_SIZE)
            as usize)
            / FSP_EXTENT_SIZE;

        let (_, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);

        for extent_no in 0..total_extents {
            let rel_off = extent_no * XDES_SIZE;
            let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
            let xdes = ExtentDescriptor::new(xdes_buf);

            free_pages += xdes.free_count(); // how many pages in this extent are marked free
        }

        free_pages
    }
}

/// Represents an extent descriptor in the InnoDB tablespace.
///
/// An extent descriptor manages a group of 64 pages (an "extent") and tracks
/// which pages are free or used. It also maintains the state of the extent
/// (free, partially free, or full) and provides methods to allocate and
/// deallocate pages within the extent.
pub struct ExtentDescriptor<'a> {
    /// The raw byte buffer containing the extent descriptor data
    buf: &'a mut [u8],
}

impl<'a> ExtentDescriptor<'a> {
    /// Creates a new `ExtentDescriptor` by wrapping a raw buffer.
    ///
    /// This method takes ownership of a mutable byte slice and provides
    /// a structured interface for manipulating it as an extent descriptor.
    ///
    /// # Arguments
    /// * `buf` - A mutable byte slice of at least XDES_SIZE bytes in length
    ///
    /// # Panics
    /// Panics if the provided buffer is smaller than XDES_SIZE
    pub fn new(buf: &'a mut [u8]) -> Self {
        assert!(
            buf.len() >= XDES_SIZE,
            "extent-descriptor buffer must be at least {} bytes",
            XDES_SIZE
        );
        ExtentDescriptor { buf }
    }

    /// In debug builds, mark the memory as invalid (for sanitizer/debug hooks).
    #[cfg(debug_assertions)]
    fn univ_mem_invalid(&mut self) {
        // Fill with a recognizable pattern (0xCC) to catch uninitialized reads
        self.buf.fill(0xCC);
    }

    /// No-op in non-debug builds
    #[cfg(not(debug_assertions))]
    fn univ_mem_invalid(&mut self) {}

    /// Initializes a file page with basic header information.
    ///
    /// This method performs the low-level initialization of a page buffer:
    /// - Zeros out the entire page
    /// - Sets the page number
    /// - Clears the LSN field
    /// - Sets the space ID
    /// - Clears the trailer (checksum) area
    ///
    /// # Arguments
    ///
    /// * `page_no` - The page number to assign to this page
    /// * `space_id` - The space ID to assign to this page
    ///
    pub fn init_file_page_low(&mut self, page_no: u32, space_id: u32) {
        // debug‐only memory invalidation
        self.univ_mem_invalid();

        // zero the entire 16 KiB page
        self.buf.fill(0);

        // write the 4-byte page number (big-endian)
        self.buf[FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4]
            .copy_from_slice(&page_no.to_be_bytes());

        // clear the 8-byte LSN
        self.buf[FIL_PAGE_LSN..FIL_PAGE_LSN + 8].fill(0);

        // write the 4-byte space-ID
        self.buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4]
            .copy_from_slice(&space_id.to_be_bytes());

        // clear the 8-byte trailer (checksum, etc.)
        let trailer_off = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
        self.buf[trailer_off..trailer_off + FIL_PAGE_END_LSN_CHKSUM].fill(0);

        // duplicate space‐ID if you want C++ fidelity
        self.buf[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4]
            .copy_from_slice(&space_id.to_be_bytes());
    }

    /// Initializes a file page with transaction logging.
    ///
    /// This method wraps the low-level page initialization in a mini-transaction
    /// and would emit a redo log record in a full implementation. It provides a
    /// higher-level interface for page initialization that ensures proper recovery.
    ///
    /// # Arguments
    ///
    /// * `page_no` - The page number to assign to this page
    /// * `space_id` - The space ID to assign to this page
    /// /* `mtr` - The mini-transaction context (commented out in current implementation) */
    ///
    pub fn init_file_page(
        &mut self,
        page_no: u32,
        space_id: u32,
        /* mtr: &mut Mtr */
    ) {
        self.init_file_page_low(page_no, space_id);
        // TODO: mlog_write_initial_log_record(&mut self.buf, MlogType::InitFilePage, mtr);
    }

    /// Initialize this extent descriptor:
    /// - set every page’s two-bit entry to `11` (free & clean)
    /// - write the 4-byte state = XDES_FREE
    pub fn xdes_init(&mut self) {
        // 1) Fill the 16-byte bitmap (bytes 24..40) with 0xFF
        for byte in &mut self.buf[XDES_BITMAP..XDES_SIZE] {
            *byte = 0xFF;
        }

        // 2) Write the 4-byte state = XDES_FREE at bytes 20..24
        let state_bytes = (XDES_FREE as u32).to_le_bytes();
        self.buf[XDES_STATE..XDES_STATE + 4].copy_from_slice(&state_bytes);

        // TODO: emit corresponding MTR log record here
    }

    /// Read one of the two bits (free or clean) for page `page_idx`.
    ///
    /// - `page_idx` is 0..63
    /// - `which` is either `XDES_FREE_BIT` (0) or `XDES_CLEAN_BIT` (1)
    pub fn get_bit(&self, page_idx: usize, which: usize) -> bool {
        // Compute the absolute bit index in the 16-byte (128-bit) bitmap
        let bit = page_idx * XDES_BITS_PER_PAGE + which;
        // Find which byte in the descriptor holds that bit
        let byte_off = XDES_BITMAP + (bit / 8);
        // And which bit position (0-7) inside that byte
        let bit_in_byte = bit % 8;
        // Mask it out and test if it's non-zero
        (self.buf[byte_off] & (1 << bit_in_byte)) != 0
    }

    /// Set or clear one of the two bits (free or clean) for page `page_idx`.
    ///
    /// - `page_idx` is 0..63
    /// - `which` is `XDES_FREE_BIT` or `XDES_CLEAN_BIT`
    /// - `val = true` means set the bit to 1, `false` clears it to 0
    pub fn set_bit(&mut self, page_idx: usize, which: usize, val: bool) {
        let bit = page_idx * XDES_BITS_PER_PAGE + which;
        let byte_off = XDES_BITMAP + (bit / 8);
        let bit_in_byte = bit % 8;
        let mask = 1 << bit_in_byte;
        if val {
            // Turn that bit on, leave others alone
            self.buf[byte_off] |= mask;
        } else {
            // Turn that bit off, leave others alone
            self.buf[byte_off] &= !mask;
        }
        // TODO: emit MTR log record for this bit change
    }

    /// Returns the count of pages in this extent that are currently in use.
    ///
    /// This method counts how many pages have their XDES_FREE_BIT cleared,
    /// indicating that they are allocated and in use.
    ///
    /// # Returns
    /// The number of pages (0-64) that are currently in use
    pub fn used_count(&self) -> usize {
        let mut used = 0;
        for page_idx in 0..FSP_EXTENT_SIZE {
            if !self.get_bit(page_idx, XDES_FREE_BIT) {
                used += 1;
            }
        }
        used
    }

    /// Returns the count of pages in this extent that are currently free.
    ///
    /// This method counts how many pages have their XDES_FREE_BIT set,
    /// indicating that they are available for allocation.
    ///
    /// # Returns
    /// The number of pages (0-64) that are currently free
    pub fn free_count(&self) -> usize {
        let mut count = 0;
        for i in 0..FSP_EXTENT_SIZE {
            if self.get_bit(i, XDES_FREE_BIT) {
                count += 1;
            }
        }
        count
    }

    /// Finds the first page index where the specified bit is set.
    ///
    /// This method searches through all pages in the extent to find the first one
    /// where the specified bit (free or clean) is set to 1.
    ///
    /// # Arguments
    ///
    /// * `which` - The bit to check, either XDES_FREE_BIT (0) or XDES_CLEAN_BIT (1)
    ///
    /// # Returns
    ///
    /// * `Some(page_idx)` with the index of the first page where the bit is set
    /// * `None` if no pages have the specified bit set
    pub fn find_bit(&self, which: usize) -> Option<usize> {
        for page_idx in 0..FSP_EXTENT_SIZE {
            if self.get_bit(page_idx, which) {
                return Some(page_idx);
            }
        }
        None
    }

    /// Reads the 4-byte descriptor state.
    ///
    /// This method retrieves the current state of the extent descriptor, which can be
    /// one of the following values:
    /// - XDES_FREE: All pages in the extent are free
    /// - XDES_FREE_FRAG: Some pages in the extent are free, some are used
    /// - XDES_FULL_FRAG: All pages in the extent are used
    /// - XDES_FSEG: The extent belongs to a specific segment
    ///
    /// # Returns
    ///
    /// The current state of the extent descriptor as a usize value
    pub fn state(&self) -> usize {
        let bytes: [u8; 4] = self.buf[XDES_STATE..XDES_STATE + 4]
            .try_into()
            .expect("buffer is at least XDES_STATE+4 bytes");
        u32::from_le_bytes(bytes) as usize
    }

    /// Writes the 4-byte descriptor state.
    ///
    /// This method updates the state of the extent descriptor to reflect its current
    /// allocation status.
    ///
    /// # Arguments
    ///
    /// * `state` - The new state to set, one of XDES_FREE, XDES_FREE_FRAG, XDES_FULL_FRAG, or XDES_FSEG
    pub fn set_state(&mut self, state: usize) {
        let state_bytes = (state as u32).to_le_bytes();
        self.buf[XDES_STATE..XDES_STATE + 4].copy_from_slice(&state_bytes);
        // TODO: emit MTR log record for state change
    }

    /// Returns true if **all** 64 pages are still free.
    pub fn is_free(&self) -> bool {
        self.state() == XDES_FREE
    }

    /// Returns true if **no** pages are free (i.e. all are used).
    pub fn is_full(&self) -> bool {
        self.state() == XDES_FULL_FRAG
    }

    /// Allocate one free page from this extent.
    ///
    /// - Finds the first free page bit.
    /// - Clears it (marking that page “used”).
    /// - Updates the descriptor state to either FULL (if now empty) or
    ///   FREE_FRAG (still has some left).
    /// - Returns the global page number = `extent_no * 64 + page_idx`.
    /// - Returns `None` if `is_full()`.
    pub fn alloc_page(&mut self, extent_no: u32) -> Option<u32> {
        // 1) Find a free page
        let page_idx = self.find_bit(XDES_FREE_BIT)?;
        // 2) Mark it used
        self.set_bit(page_idx, XDES_FREE_BIT, false);
        // 3) Recompute and write new state
        let used = self.used_count();
        let new_state = if used == FSP_EXTENT_SIZE {
            XDES_FULL_FRAG
        } else {
            XDES_FREE_FRAG
        };
        self.set_state(new_state);
        // 4) Compute and return the global page number
        Some(extent_no * (FSP_EXTENT_SIZE as u32) + page_idx as u32)
    }
}
