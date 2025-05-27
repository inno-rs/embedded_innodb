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
use crate::innodb::innodb0types::UNIV_PAGE_SIZE;

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

pub struct Fsp {}

impl Fsp {
    fn new() -> Fsp {
        Fsp {}
    }
}

/// A full 16 KiB page buffer (e.g. page 0 of a space).
pub struct PageFrame<'a> {
    buf: &'a mut [u8],
}

#[cfg(test)]
impl<'a> PageFrame<'a> {
    pub fn get_buf_mut(&mut self) -> &mut [u8] {
        &mut self.buf
    }
}

impl<'a> PageFrame<'a> {
    /// Wrap a raw 16 KiB buffer.
    pub fn new(buf: &'a mut [u8]) -> Self {
        assert!(
            buf.len() >= UNIV_PAGE_SIZE,
            "PageFrame needs a full UNIV_PAGE_SIZE buffer"
        );
        PageFrame { buf }
    }

    /// Initialize the FSP header in this page (port of FSP::header_init).
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

    /** Allocates a free extent from the FSP_FREE list, marks it as partially used,
    and moves it to the FSP_FREE_FRAG list.

    This function will:
    •	Pick one free extent from the free list
    •	Mark it as “partially used”
    •	Move it to the “fragmented list”
    •	Return its index


    */
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
}

pub struct ExtentDescriptor<'a> {
    buf: &'a mut [u8],
}

impl<'a> ExtentDescriptor<'a> {
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

    /// Wraps the low‐level init in a mini‐trx and emits a redo‐log stub
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

    pub fn used_count(&self) -> usize {
        let mut used = 0;
        for page_idx in 0..FSP_EXTENT_SIZE {
            if !self.get_bit(page_idx, XDES_FREE_BIT) {
                used += 1;
            }
        }
        used
    }

    /// Find the first page index where `which` bit is set, or None if none found.
    pub fn find_bit(&self, which: usize) -> Option<usize> {
        for page_idx in 0..FSP_EXTENT_SIZE {
            if self.get_bit(page_idx, which) {
                return Some(page_idx);
            }
        }
        None
    }

    /// Read the 4-byte descriptor state (FULL, FREE, FREE_FRAG...).
    pub fn state(&self) -> usize {
        let bytes: [u8; 4] = self.buf[XDES_STATE..XDES_STATE + 4]
            .try_into()
            .expect("buffer is at least XDES_STATE+4 bytes");
        u32::from_le_bytes(bytes) as usize
    }

    /// Write the 4-byte descriptor state.
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
