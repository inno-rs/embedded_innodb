//! On-disk layout constants for InnoDB FSP
//!

use crate::fil::fil0types::{
    FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN, FIL_PAGE_OFFSET, FIL_PAGE_SPACE_ID,
};
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

/// In debug builds, mark the memory as invalid (for sanitizer/debug hooks).
#[cfg(debug_assertions)]
fn univ_mem_invalid(frame: &mut [u8]) {
    // Fill with a recognizable pattern (0xCC) to catch uninitialized reads
    frame.fill(0xCC);
}

/// No-op in non-debug builds
#[cfg(not(debug_assertions))]
fn univ_mem_invalid(_frame: &mut [u8]) {}

// Page initialization routines
/// Wraps `init_file_page_low` in a mini-transaction and logs the operation
/// Direct port of InnoDB's `fsp_init_file_page_low`
///
/// https://docs.google.com/document/d/17Cv1R1ELip8PYuCrQFgbkxe1KRfgsjWFyCje9bS9dRI/edit?tab=t.0#heading=h.kiodsa4o1rj7
pub fn init_file_page_low(frame: &mut [u8], page_no: u32, space_id: u32) {
    univ_mem_invalid(frame);

    // -- Zero the entire page
    frame.fill(0);

    // -- Write page number (4 bytes, big-endian)
    //We’re choosing big-endian because the on-disk InnoDB format
    // defines the page-number and space-ID fields as big-endian values.
    frame[FIL_PAGE_OFFSET..FIL_PAGE_OFFSET + 4]
        .copy_from_slice(&page_no.to_be_bytes());

    // -- Zero the 8-byte page LSN
    // Not sure why do we need to zero them again!!!
    frame[FIL_PAGE_LSN..FIL_PAGE_LSN + 8].fill(0);

    // -- Write space ID (4 bytes, big-endian)
    frame[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4]
        .copy_from_slice(&space_id.to_be_bytes());

    // -- Zero the 8-byte end-LSN/checksum trailer
    let trailer_start = UNIV_PAGE_SIZE - FIL_PAGE_END_LSN_CHKSUM;
    frame[trailer_start..trailer_start + FIL_PAGE_END_LSN_CHKSUM].fill(0);

    // -- (Optional) Repeat space ID if mirroring C++ duplication
    frame[FIL_PAGE_SPACE_ID..FIL_PAGE_SPACE_ID + 4]
        .copy_from_slice(&space_id.to_be_bytes());
}

// Wrapper of  method init_file_page_low().
pub fn init_file_page(
    frame: &mut [u8],
    page_no: u32,
    space_id: u32,
    /* mtr: &mut Mtr */
) {
    init_file_page_low(frame, page_no, space_id);
    // TODO: emit redo-log record using `mlog_write_initial_log_record`
}

/// Initialize one extent descriptor (40 bytes):
/// - bytes 24..40: set every byte to 0xFF → all 64 pages “free & clean”
/// - bytes 20..24: write the 4‐byte state = XDES_FREE (little‐endian)
pub fn xdes_init(descr: &mut [u8]) {
    assert!(
        descr.len() >= XDES_SIZE,
        "extent-descriptor buffer must be at least {} bytes",
        XDES_SIZE
    );
    // Byte 24- Byte 39 ( State of pages bitmap within extent)
    for i in XDES_BITMAP..XDES_SIZE {
        descr[i] = 0xFF;
    }

    let state_bytes = (XDES_FREE as u32).to_le_bytes();
    let off = XDES_STATE;

    // Byte 20 -23(4 bytes) is the state of extent
    descr[off..off + 4].copy_from_slice(&state_bytes);
    // TODO: emit corresponding MTR log records here
}

/// Read a single bit from the extent descriptor bitmap
/// `page_idx` is 0..63, `which` is either XDES_FREE_BIT or XDES_CLEAN_BIT
///
/// TODO test pending
pub fn xdes_get_bit(descr: &[u8], page_idx: usize, which: usize) -> bool {
    let bit = page_idx * XDES_BITS_PER_PAGE + which;
    let byte_off = XDES_BITMAP + (bit / 8);
    let bit_in_byte = bit % 8;
    (descr[byte_off] & (1 << bit_in_byte)) != 0
}

/// Set or clear a single bit in the extent descriptor bitmap
///
/// TODO test pending
pub fn xdes_set_bit(
    descr: &mut [u8],
    page_idx: usize,
    which: usize,
    val: bool,
) {
    let bit = page_idx * XDES_BITS_PER_PAGE + which;
    let byte_off = XDES_BITMAP + (bit / 8);
    let bit_in_byte = bit % 8;
    if val {
        descr[byte_off] |= 1 << bit_in_byte;
    } else {
        descr[byte_off] &= !(1 << bit_in_byte);
    }
}

/// Count how many pages are currently in-use (free-bit == 0)
pub fn xdes_get_n_used(descr: &[u8]) -> usize {
    let mut used = 0;
    for page_idx in 0..FSP_EXTENT_SIZE {
        if !xdes_get_bit(descr, page_idx, XDES_FREE_BIT) {
            used += 1;
        }
    }
    used
}

/// Find the first page index where `which` bit is set, or None if none found
/// It returns the option usize
pub fn xdes_find_bit(descr: &[u8], which: usize) -> Option<usize> {
    for page_idx in 0..FSP_EXTENT_SIZE {
        if xdes_get_bit(descr, page_idx, which) {
            return Some(page_idx);
        }
    }
    None
}

/// Read the 4-byte descriptor state (FULL, FREE, FREE_FRAG...)
pub fn xdes_get_state(descr: &[u8]) -> usize {
    let bytes: [u8; 4] =
        descr[XDES_STATE..XDES_STATE + 4].try_into().expect("length checked");
    u32::from_le_bytes(bytes) as usize
}

/// Write the 4-byte descriptor state
pub fn xdes_set_state(
    descr: &mut [u8],
    state: usize, /*, mtr: &mut Mtr */
) {
    let state_bytes = (state as u32).to_le_bytes();
    descr[XDES_STATE..XDES_STATE + 4].copy_from_slice(&state_bytes);
    // TODO: emit MTR log
}
