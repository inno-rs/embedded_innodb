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
// Page-offset  Length  Field  	Description
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

use std::{
    ops::Range,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU32, Ordering},
    },
};

use crate::{
    buf::{
        buf0buf::BufPool,
        buf0types::{BUF_GET_ALWAYS, BufBlock, Request},
    },
    fil::{
        fil0fil::Fil,
        fil0types::{
            FIL_PAGE_DATA, FIL_PAGE_END_LSN_CHKSUM, FIL_PAGE_LSN,
            FIL_PAGE_OFFSET, FIL_PAGE_SPACE_ID, FIL_PAGE_TYPE, FILL_NULL,
            FilPageType,
        },
    },
    fsp::fsp0types::{Direction, ExtentNo, FSegId},
    fut::fut0lst::{FLst, FLstBaseNode, FilAddr},
    innodb::innodb0types::{
        Frame, PageFrame as InnoPageFrame, PageId, PageNo, SpaceId,
        UNIV_PAGE_SIZE,
    },
    mach::mach0data::{
        mach_read_from_4, mach_read_from_8, mach_write_to_4, mach_write_to_8,
    },
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{FixMode, MemoSlot, MlogType},
    },
    page::page0page::Page,
    some_or_panic,
    sync::sync0rw::ReadWriteLatch,
    ut::ut0dbg::ut_ad,
};

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
    bits.div_ceil(8)
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
pub struct Fsp {
    /// The buffer pool used for managing page buffers.
    buf_pool: Arc<Mutex<BufPool>>,
    /// Reference to fil.
    fil: Arc<Mutex<Fil>>,
    /// The page number of the next page to allocate.
    page_no: AtomicU32,
}

impl Fsp {
    /// Creates a new file space manager instance.
    ///
    /// Returns an empty `Fsp` struct that can be used to manage tablespace operations.
    pub fn new(buf_pool: Arc<Mutex<BufPool>>, fil: Arc<Mutex<Fil>>) -> Fsp {
        Fsp { buf_pool, fil, page_no: AtomicU32::new(1) }
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
        mach_write_to_4(&mut buf[FSP_SPACE_ID_RANGE], space_id);
        mach_write_to_4(&mut buf[FSP_SPACE_FLAGS_RANGE], flags as u32);
    }

    /// Retrieves the space header block for a given tablespace.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the tablespace to retrieve the header for
    /// * `mtr` - A mutable reference to the multi-versioning transaction record
    ///
    /// # Returns
    ///
    /// An `Arc<RwLock<BufBlock>>` containing the space header block.
    ///
    pub fn get_space_header_block(
        &self,
        space_id: SpaceId,
        mtr: &mut Mtr,
    ) -> Option<Arc<RwLock<BufBlock>>> {
        let buf_block = self.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::XLatch,
            page_id: PageId { space_id, page_no: 0 },
            mode: BUF_GET_ALWAYS,
            mtr,
        });

        if let Some(buf_block) = buf_block {
            let block_read = buf_block.read().unwrap();
            let frame = block_read.get_page_frame();

            ut_ad!(block_read.is_frame_x_latched());
            ut_ad!(
                some_or_panic!(Self::get_space_header_from_page(frame))
                    == space_id
            );

            return Some(buf_block.clone());
        }

        None
    }

    /// Extracts the space ID from a given page frame.
    ///
    /// # Arguments
    ///
    /// * `page_frame` - The page frame from which to extract the space ID.
    ///
    /// # Returns
    ///
    /// An `Option<SpaceId>` containing the space ID if it can be read, or `None` if the page frame is invalid.
    ///
    pub fn get_space_header_from_page(
        page_frame: InnoPageFrame,
    ) -> Option<SpaceId> {
        Some(mach_read_from_4(&page_frame.0[FSP_SPACE_ID_RANGE]))
    }

    /// Retrieves the header page for a given tablespace.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The ID of the tablespace to retrieve the header for
    /// * `mtr` - A mutable reference to the multi-versioning transaction record
    ///
    /// # Returns
    ///
    /// A `Page` representing the header page of the specified tablespace.
    ///
    fn get_header_page(
        &self,
        space_id: SpaceId,
        mtr: &mut Mtr,
    ) -> Option<Page> {
        let header_block = self.get_space_header_block(space_id, mtr);

        if let Some(header_block) = header_block {
            let read_guard = header_block.read().unwrap();
            ut_ad!(read_guard.is_frame_x_latched());

            return Some(read_guard.get_page());
        }

        None
    }

    pub fn header_init(
        &self,
        space_id: SpaceId,
        space_size: u32,
        mtr: &mut Mtr,
    ) {
        let header_page = self.get_header_page(space_id, mtr);
        if let Some(header_page) = header_page {
            let mut fsp_header_page =
                FspHeaderPage::new(header_page.to_page_frame().0);
            fsp_header_page.header_init(space_id, space_size);
        }
    }

    /// Allocates a single free page from the segment. We try to minimize page fragmentation.
    pub fn fseg_alloc_free_page_general(
        &self,
        space_id: SpaceId,
        hint: PageNo,
        direction: Direction,
        reserved: bool,
        mtr: &mut Mtr,
    ) -> PageNo {
        // let fil_space = {
        //     let fil = self.fil.lock().unwrap();
        //     some_or_panic!(fil.space_get_by_id(space_id))
        // };

        let page_no =
            self.page_no.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if let Some(buf_block) = self.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::NoLatch,
            page_id: PageId { space_id, page_no },
            mode: BUF_GET_ALWAYS,
            mtr,
        }) {
            let mut page = buf_block.write().unwrap().get_page();
            page.page_set_space_id(space_id);
            page.page_set_fil_offset(page_no as usize);
        }

        page_no
    }

    /// Allocates a segment inode page and adds it to the free list of segment inode pages.
    ///
    /// # Arguments
    ///
    /// * `fsp_header_page`: A mutable reference to the FSP header page for the space id.
    /// * `space_id`: The ID of the tablespace to allocate the segment inode page in.
    /// * `hint`: A hint for the page number to allocate.
    /// * `mtr`: A mutable reference to the mini transaction.
    ///
    /// # Returns
    ///
    /// `bool` - Returns `true` if the allocation was successful, `false` otherwise.
    ///
    pub fn alloc_seg_inode_page(
        &self,
        fsp_header_page: &mut FspHeaderPage,
        space_id: SpaceId,
        hint: PageNo,
        mtr: &mut Mtr,
    ) -> bool {
        let space_id = fsp_header_page.fsp_get_space_id();
        let page_no = self.alloc_free_page(space_id, hint, mtr);

        if page_no.is_none() {
            return false;
        }

        let page_no = some_or_panic!(page_no);

        let block = self.buf_pool.lock().unwrap().get(Request {
            rw_latch: ReadWriteLatch::XLatch,
            page_id: PageId { space_id, page_no },
            mode: BUF_GET_ALWAYS,
            mtr,
        });

        let buf_block = some_or_panic!(block);
        let page = buf_block.read().unwrap().get_page();

        mtr.write_u32(
            &page,
            FIL_PAGE_TYPE,
            FilPageType::Inode as u32,
            MlogType::MLog2Bytes,
        );

        let mut fseg_inode_page = FSegInodePage::new(page);
        for i in 0..FSP_SEG_INODES_PER_PAGE {
            let mut fseg_inode =
                fseg_inode_page.fsp_seg_inode_page_get_nth_inode(i, mtr);
            // todo: mtr log
            // mtr.write_uint64()
            fseg_inode.fseg_inode_set_fseg_id(FSegId(0));
        }

        self.add_to_fseg_inode_free_list_last(
            fsp_header_page,
            &mut fseg_inode_page,
            mtr,
        );

        true
    }

    pub fn alloc_free_page(
        &self,
        space_id: SpaceId,
        hint_page_no: PageNo,
        mtr: &mut Mtr,
    ) -> Option<PageNo> {
        Some(self.page_no.fetch_add(1, Ordering::Relaxed))
    }

    /// Adds a segment inode page to the free list of segment inode pages.
    ///
    /// # Arguments
    ///
    /// * `fsp_header_page` - The FSP header page containing the free list
    /// * `fseg_inode_page` - The segment inode page to add
    /// * `mtr` - A mutable reference to mini transaction
    ///
    pub fn add_to_fseg_inode_free_list_last(
        &self,
        fsp_header_page: &mut FspHeaderPage,
        fseg_inode_page: &mut FSegInodePage,
        mtr: &mut Mtr,
    ) {
        let space_id = fsp_header_page.fsp_get_space_id();
        let mut fseg_inode_free_list =
            fsp_header_page.fsp_get_seg_inodes_free_list_mut();
        let last = fseg_inode_free_list.get_last();
        if last == FilAddr::NULL {
            let fil_addr = FilAddr {
                page_no: fseg_inode_page.get_page_no(),
                byte_off: FSEG_INODE_PAGE_NODE as u16,
            };

            fseg_inode_free_list.set_last(fil_addr);
            fseg_inode_free_list.set_first(fil_addr);

            fseg_inode_page.set_prev_page(FILL_NULL);
            fseg_inode_page.set_next_page(FILL_NULL);
        } else {
            let last_page_no = last.page_no;
            let last_byte_off = last.byte_off;
            let last_block = self.buf_pool.lock().unwrap().get(Request {
                rw_latch: ReadWriteLatch::NoLatch,
                page_id: PageId { space_id, page_no: last_page_no },
                mode: BUF_GET_ALWAYS,
                mtr,
            });

            let last_buf_page = some_or_panic!(last_block);
            let last_page = last_buf_page.read().unwrap().get_page();

            fseg_inode_page.set_prev_page(last_page_no);
            last_page.set_next_page(fseg_inode_page.get_page_no());

            fseg_inode_free_list.set_last(FilAddr {
                page_no: fseg_inode_page.get_page_no(),
                byte_off: FSEG_INODE_PAGE_NODE as u16,
            });
        }

        fseg_inode_free_list.incr_length();
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
/// various fields on the page according to InnoDB's on-disk format.
pub struct FspHeaderPage<'a> {
    /// The raw byte buffer representing the page
    buf: &'a mut [u8; UNIV_PAGE_SIZE],
    page: Page,
}

#[cfg(test)]
impl<'a> FspHeaderPage<'a> {
    pub fn get_buf_mut(&mut self) -> &mut [u8; UNIV_PAGE_SIZE] {
        ut_ad!(self.buf.as_ptr() as usize % UNIV_PAGE_SIZE == 0);

        self.buf
    }
}

impl<'a> FspHeaderPage<'a> {
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
    pub fn new(buf: &'a mut [u8; UNIV_PAGE_SIZE]) -> Self {
        ut_ad!(
            buf.len() >= UNIV_PAGE_SIZE,
            "PageFrame needs a full UNIV_PAGE_SIZE buffer"
        );
        let page = Page::new(buf.as_mut_ptr());
        FspHeaderPage { buf, page }
    }

    pub fn get_page_no(&self) -> PageNo {
        self.page.page_get_fil_offset()
    }

    /// Get space ID from the FSP header.
    ///
    /// # Returns
    ///
    /// `SpaceId` - FSP space ID
    /// .
    pub fn fsp_get_space_id(&self) -> SpaceId {
        mach_read_from_4(&self.buf[FSP_SPACE_ID_RANGE])
    }

    /// Sets the space ID in the FSP header.
    ///
    /// # Arguments
    ///
    /// * `space_id` - SpaceId to write.
    ///
    pub fn fsp_set_space_id(&mut self, space_id: SpaceId) {
        // Byte 38 to Byte 41
        mach_write_to_4(&mut self.buf[FSP_SPACE_ID_RANGE], space_id);
    }

    pub fn fsp_get_fsp_size(&self) -> u32 {
        mach_read_from_4(
            &self.buf[FSP_HEADER_OFFSET + FSP_SIZE
                ..FSP_HEADER_OFFSET + FSP_SIZE + 4],
        )
    }

    pub fn fsp_set_fsp_size(&mut self, fsp_size: u32) {
        // Byte 46 to 49
        mach_write_to_4(
            &mut self.buf[FSP_HEADER_OFFSET + FSP_SIZE
                ..FSP_HEADER_OFFSET + FSP_SIZE + 4],
            fsp_size,
        );
    }

    pub fn fsp_get_fsp_free_limit(&self) -> u32 {
        mach_read_from_4(
            &self.buf[FSP_HEADER_OFFSET + FSP_FREE_LIMIT
                ..FSP_HEADER_OFFSET + FSP_FREE_LIMIT + 4],
        )
    }

    pub fn fsp_set_fsp_free_limit(&mut self, limit: u32) {
        // Byte 50 to 53
        mach_write_to_4(
            &mut self.buf[FSP_HEADER_OFFSET + FSP_FREE_LIMIT
                ..FSP_HEADER_OFFSET + FSP_FREE_LIMIT + 4],
            limit,
        );
    }

    /// Retrieves the free list base node for the FSP_FREE list.
    ///
    /// # Returns
    ///
    /// `FLstBaseNode` - The base node of the free list, which contains
    /// the first and last addresses of free extents.
    ///
    pub fn fsp_get_seg_inodes_free_list_mut(&mut self) -> FLst<'_> {
        let base_node = FLstBaseNode::new(
            FilAddr { page_no: 0, byte_off: FSP_SEG_INODES_FREE as u16 },
            Frame(
                <&mut [u8; FLST_BASE_NODE_SIZE]>::try_from(
                    &mut self.buf[(FSP_HEADER_OFFSET + FSP_SEG_INODES_FREE)
                        ..(FSP_HEADER_OFFSET
                            + FSP_SEG_INODES_FREE
                            + FLST_BASE_NODE_SIZE)],
                )
                .unwrap(),
            ),
        );

        FLst::new(base_node)
    }

    /// Initializes the FSP (File Space) header in this page.
    ///
    /// This method sets up the file space header at the beginning of the page,
    /// which includes space ID, size information, and initializes the linked lists
    /// that track free, partially free, and full extents.
    ///
    /// # Arguments
    ///
    /// * `space_id` - The unique identifier for this tablespace
    /// * `space_size` - The initial size of the tablespace in pages
    ///
    pub fn header_init(&mut self, space_id: u32, space_size: u32) {
        let start = FSP_HEADER_OFFSET; // Byte 38
        let end = start + FSP_HEADER_SIZE; // Byte 150 (Total 112 Bytes)
        self.buf[start..end].fill(0);

        self.fsp_set_space_id(space_id);
        self.fsp_set_fsp_size(space_size);
        self.fsp_set_fsp_free_limit(space_size);

        // Flags and frag_n_used are already zeroed

        // Below are 5 linked lists
        let heads = [
            FSP_FREE,            // free list Byte 62-77 [38 + 24 -- 38+24+16]
            FSP_FREE_FRAG,       // partially free extents // Byte 78-93
            FSP_FULL_FRAG,       // list of full extents // Byte 94-109
            FSP_SEG_INODES_FULL, // or “full” segment‐inode pages
            FSP_SEG_INODES_FREE, // for “free” segment‐inode pages
        ];
        for &off in &heads {
            let abs = start + off;
            let mut base_node = FLstBaseNode::new(
                FilAddr { page_no: 0, byte_off: FSP_FREE as u16 },
                Frame(
                    <&mut [u8; FLST_BASE_NODE_SIZE]>::try_from(
                        &mut self.buf[abs..abs + FLST_BASE_NODE_SIZE],
                    )
                    .unwrap(),
                ),
            );

            base_node.set_last(FilAddr::NULL);
            base_node.set_first(FilAddr::NULL);
        }

        mach_write_to_4(
            &mut self.buf[start + FSP_SEG_ID..start + FSP_SEG_ID + 4],
            0,
        );
    }

    /// Allocates a free extent from the FSP_FREE list, marks it as partially used,
    /// and moves it to the FSP_FREE_FRAG list.
    ///
    /// This function will:
    /// • Pick one free extent from the free list
    /// • Mark it as “partially used”
    /// • Move it to the “fragmented list”
    /// • Return its index
    ///
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

        // Step 2: Split the buffer into FSP header + extent descriptor area
        let (header_area, xdes_area) = buf.split_at_mut(XDES_ARR_OFFSET);

        // Step 3: Get a mutable slice for the 40-byte extent descriptor
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];

        // Step 4: Initialize the extent descriptor and mark as partially used
        let mut xdes = ExtentDescriptor::new(xdes_buf);
        xdes.xdes_init();
        xdes.set_state(XDES_FREE_FRAG);

        // Step 5: Remove from FSP_FREE list (use header_part safely now)
        {
            let free_head = &mut header_area[FSP_HEADER_OFFSET + FSP_FREE..];
            first.remove(free_head, xdes_buf, None, None);
        }

        // Step 6: Add to FSP_FREE_FRAG list
        {
            let free_frag_head =
                &mut header_area[FSP_HEADER_OFFSET + FSP_FREE_FRAG..];
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
    ///
    /// * `page_no` - The page number to free
    ///
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
    ///
    /// * `Some(page_no)` - The page number of the allocated page
    /// * `None` - If no free page could be allocated
    ///
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
    1.	Try to find a partially used extent (a FREE_FRAG extent) and allocate a page from it.
    2.	If no such extent exists, allocate a brand-new extent (FREE extent),
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
        // 1. Read the current space size
        let current_pages = self.fsp_get_fsp_size();

        // 2. Determine a new extent number and its byte offset
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
        self.fsp_set_fsp_size(new_size);

        Some(new_extent_no)
    }

    pub fn is_page_free(&mut self, page_no: u32) -> bool {
        let extent_no = page_no / FSP_EXTENT_SIZE as u32;
        let page_idx = (page_no % FSP_EXTENT_SIZE as u32) as usize;
        let extent_off = XDES_ARR_OFFSET + extent_no as usize * XDES_SIZE;

        let xdes = self.get_extent_descriptor(ExtentNo(extent_no));

        xdes.get_bit(page_idx, XDES_FREE_BIT)
    }

    /// Check if the extent is used.
    ///
    /// # Arguments
    ///
    /// * `extent_no`: Extent number to check.
    ///
    /// # Returns
    ///
    /// `true` if the extent is used (has at least one page allocated), `false` otherwise.
    ///
    pub fn is_extent_used(&mut self, extent_no: ExtentNo) -> bool {
        let xdes = self.get_extent_descriptor(extent_no);
        xdes.used_count() > 0
    }

    pub fn space_available(&mut self) -> usize {
        let mut free_pages = 0;
        let total_extents =
            (self.fsp_get_fsp_size()) as usize / FSP_EXTENT_SIZE;

        for extent_no in 0..total_extents {
            let xdes = self.get_extent_descriptor(ExtentNo(extent_no as u32));

            free_pages += xdes.free_count(); // Number of pages marked as free in this extent
        }

        free_pages
    }

    pub fn get_extent_descriptor(
        &mut self,
        extent_no: ExtentNo,
    ) -> ExtentDescriptor<'_> {
        let extent_off = XDES_ARR_OFFSET + extent_no.0 as usize * XDES_SIZE;
        let (_, xdes_area) = self.buf.split_at_mut(XDES_ARR_OFFSET);
        let rel_off = extent_off - XDES_ARR_OFFSET;
        let xdes_buf = &mut xdes_area[rel_off..rel_off + XDES_SIZE];
        ExtentDescriptor::new(xdes_buf)
    }

    fn get_extent_pair(
        buf: &'a mut [u8],
        i: usize,
        j: usize,
    ) -> (ExtentDescriptor<'a>, ExtentDescriptor<'a>) {
        assert!(i < j, "Expected i < j");
        assert!((j + 1) * XDES_SIZE <= buf.len(), "Out of bounds");

        // Split at start of j-th descriptor
        let (left, right) = buf.split_at_mut(j * XDES_SIZE);

        let desc_i = ExtentDescriptor::new(
            &mut left[i * XDES_SIZE..(i + 1) * XDES_SIZE],
        );
        let desc_j = ExtentDescriptor::new(&mut right[0..XDES_SIZE]);

        (desc_i, desc_j)
    }
}

/// Enum representing the possible states of an extent descriptor.
#[repr(u8)]
pub enum XDesState {
    /// XDES_FREE
    Free = 1,
    /// XDES_FREE_FRAG
    FreeFrag = 2,
    /// XDES_FULL_FRAG
    FullFrag = 3,
    /// XDES_FSEG
    Fseg = 4,
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
    /// # Panic
    /// Panics if the provided buffer is smaller than XDES_SIZE
    pub fn new(buf: &'a mut [u8]) -> Self {
        ut_ad!(
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
    pub fn init_file_page_low(&mut self, page_no: PageNo, space_id: SpaceId) {
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
    /// * `mtr` - The mini-transaction context (commented out in the current implementation)
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
        self.buf[XDES_BITMAP..XDES_SIZE].fill(0xFF);

        // 2) Write the 4-byte state = XDES_FREE at bytes 20..24
        self.set_state(XDES_FREE);

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
    ///
    /// The number of pages (0-64) that are currently in use
    ///
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
    ///
    pub fn find_bit(&self, which: usize) -> Option<usize> {
        (0..FSP_EXTENT_SIZE).find(|&page_idx| self.get_bit(page_idx, which))
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
    /// The current state of the extent descriptor as an usize value
    ///
    pub fn state(&self) -> usize {
        mach_read_from_4(&self.buf[XDES_STATE..XDES_STATE + 4]) as usize
    }

    /// Writes the 4-byte descriptor state.
    ///
    /// This method updates the state of the extent descriptor to reflect its current
    /// allocation status.
    ///
    /// # Arguments
    ///
    /// * `state` - The new state to set, one of XDES_FREE, XDES_FREE_FRAG, XDES_FULL_FRAG, or XDES_FSEG
    ///
    pub fn set_state(&mut self, state: usize) {
        mach_write_to_4(
            &mut self.buf[XDES_STATE..XDES_STATE + 4],
            state as u32,
        );
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

/// Represents a single **file segment inode** (`fseg_inode_t`)
/// inside an InnoDB inode page (`FIL_PAGE_TYPE_INODE`).
///
/// Each inode entry is a fixed-size structure of [`FSEG_INODE_SIZE`] bytes
/// (192 bytes), which tracks the extents and fragment pages owned by
/// a single file segment (e.g. a B-tree leaf segment, non-leaf segment,
/// or a BLOB column segment).
///
/// # Layout
///
/// An `fseg_inode_t` on the disk contains:
///
/// - **Three extent list heads** (`FREE`, `NOT_FULL`, `FULL`)
///   Each is a [`flst_base_node_t`] (12 bytes) pointing into
///   [`xdes_t`] extent descriptors owned by this segment.
///
/// - **Segment ID** (`FSEG_ID`, 8 bytes)  
///   Uniquely identifies the file segment.
///   `0` means unused/free.
///
/// - **Reserved / Magic (4 bytes)**  
///   Internal marker field.
///
/// - **Fragment page array (`frag_arr`, 32 × 4 = 128 bytes)**  
///   Tracks up to 32 individually allocated pages from
///   fragment extents (for small segments).
///
/// Total size = 192 bytes = [`FSEG_INODE_SIZE`].
///
pub struct FSegInode<'a> {
    buf: Frame<'a, FSEG_INODE_SIZE>,
}

impl<'a> FSegInode<'a> {
    pub fn new(buf: Frame<'a, FSEG_INODE_SIZE>) -> Self {
        FSegInode { buf }
    }

    /// Initialize the inode entry.
    ///
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    pub fn init(&mut self, mtr: &mut Mtr) {
        // Go through all the slots and set the fragment slot to FILL_NULL.
        for i in 0..FSEG_FRAG_ARR_N_SLOTS {
            self.fseg_set_nth_frag_page_no(i, FILL_NULL, mtr);
        }
    }

    /// Get the segment inode ID from the inode buffer.
    ///
    /// This method reads the first 8 bytes of the inode buffer,
    /// which contains the segment ID.
    ///
    /// # Returns
    ///
    /// The segment ID as a 64-bit unsigned integer.
    ///
    pub fn fseg_inode_get_fseg_id(&self) -> u64 {
        mach_read_from_8(self.buf.0)
    }

    /// Set the segment inode ID in the inode buffer.
    ///
    /// # Arguments
    ///
    /// * `fseg_id` - The segment ID to set, represented as a `FSegId`.
    ///
    pub fn fseg_inode_set_fseg_id(&mut self, fseg_id: FSegId) {
        mach_write_to_8(self.buf.0, fseg_id.0);
    }

    /// Get the page number from the nth fragment page slot.
    ///
    /// # Arguments
    ///
    /// * `n`: The index of the fragment page slot (0-based).
    ///
    /// # Returns
    ///
    /// `PageNo` at the nth frag position.
    pub fn fseg_get_nth_frag_page_no(&self, n: usize) -> PageNo {
        ut_ad!(n < FSEG_FRAG_ARR_N_SLOTS);

        let start = FSEG_ARR_OFFSET + n * FSEG_FRAG_SLOT_SIZE;
        let end = start + FSEG_FRAG_SLOT_SIZE;
        mach_read_from_4(&self.buf.0[start..end])
    }

    /// Set the page number for the nth fragment page slot.
    ///
    /// # Arguments
    ///
    /// * `n`: The index of the fragment page slot (0-based).
    /// * `page_no`: The page no to write to the slot.
    /// * `mtr` - Mini-transaction for logging the operation.
    ///
    pub fn fseg_set_nth_frag_page_no(
        &mut self,
        n: usize,
        page_no: PageNo,
        mtr: &mut Mtr,
    ) {
        ut_ad!(n < FSEG_FRAG_ARR_N_SLOTS);

        let start = FSEG_ARR_OFFSET + n * FSEG_FRAG_SLOT_SIZE;
        let end = start + FSEG_FRAG_SLOT_SIZE;
        mach_write_to_4(&mut self.buf.0[start..end], page_no);

        // mtr.write_u32()
    }

    /// Find a free fragment page slot.
    ///
    /// # Returns
    ///
    /// `Some(usize)` is the index of the first free fragment page slot,
    /// or `None` if all slots are used.
    ///
    pub fn fseg_find_free_frag_page_slot(&self) -> Option<usize> {
        (0..FSEG_FRAG_ARR_N_SLOTS)
            .find(|&i| self.fseg_get_nth_frag_page_no(i) == FILL_NULL)
    }

    /// Find a fragment page slot which is used and last in the array.
    ///
    /// # Returns
    ///
    /// `Some(uszie)` is the index of the first free fragment page slot,
    /// or `None` if all slots are used.
    ///
    pub fn fseg_find_last_used_frag_page_slot(&self) -> Option<usize> {
        (0..FSEG_FRAG_ARR_N_SLOTS)
            .rev()
            .find(|&i| self.fseg_get_nth_frag_page_no(i) != FILL_NULL)
    }

    /// Calculate reserved fragment page slots.
    ///
    /// # Returns
    ///
    /// `usize` total count of reserved fragment page slots.
    pub fn fseg_get_n_frag_pages(&self) -> usize {
        (0..FSEG_FRAG_ARR_N_SLOTS)
            .filter(|&i| self.fseg_get_nth_frag_page_no(i) != FILL_NULL)
            .count()
    }

    /// Get all the frag pages from the inode slot.
    #[cfg(test)]
    pub fn fseg_get_all_frag_pages(&self) -> Vec<PageNo> {
        (0..FSEG_FRAG_ARR_N_SLOTS)
            .map(|i| self.fseg_get_nth_frag_page_no(i))
            .collect::<Vec<_>>()
    }
}

/// Represents a ** InnoDB inode page ** (`FIL_PAGE_TYPE_INODE`).
///
/// An inode page stores multiple [`fseg_inode_t`] entries (each 192 bytes),
/// which describe file segments belonging to B‑trees or BLOB columns.
///
/// # Layout
///
/// Each inode page is 16 KB (`UNIV_PAGE_SIZE`) and consists of:
///
/// - The standard page header (`FIL_PAGE_HEADER`)
/// - An array of [`fseg_inode_t`] entries (≈85 entries per page)
///
pub struct FSegInodePage {
    page: Page,
}

impl FSegInodePage {
    pub fn new(page: Page) -> Self {
        Self { page }
    }

    pub fn get_prev_page(&self) -> PageNo {
        self.page.get_prev_page()
    }

    pub fn set_prev_page(&mut self, prev_page: PageNo) {
        self.page.set_prev_page(prev_page);
    }

    pub fn set_next_page(&mut self, next_page: PageNo) {
        self.page.set_next_page(next_page);
    }

    pub fn get_next_page(&self) -> PageNo {
        self.page.get_next_page()
    }

    pub fn get_page_no(&self) -> PageNo {
        self.page.page_get_fil_offset()
    }

    /// Retrieves the nth inode from the segment inode page.
    ///
    /// # Arguments
    ///
    /// * `n` - The index of the inode to retrieve (0-based)
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// A `FsegInode` representing the nth inode in the segment inode page.
    ///
    pub fn fsp_seg_inode_page_get_nth_inode(
        &self,
        n: usize,
        mtr: &Mtr,
    ) -> FSegInode<'_> {
        ut_ad!(n < FSP_SEG_INODES_PER_PAGE);
        ut_ad!(mtr.memo_contains(&MemoSlot::PageFix(
            self.page.page_get_page_id(),
            FixMode::Exclusive
        )));

        let page_frame = self.page.to_page_frame();
        let start = FSEG_ARR_OFFSET + n * FSEG_INODE_SIZE;
        let end = start + FSEG_INODE_SIZE;
        let frame = Frame(
            <&mut [u8; 192]>::try_from(&mut page_frame.0[start..end]).unwrap(),
        );

        FSegInode { buf: frame }
    }

    /// Find an unused segment inode on a segment inode page.
    ///
    /// # Arguments
    ///
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// `Some(n)` where `n` is the index of the first unused inode, or `None` if all inodes are used.
    ///
    pub fn fsp_seg_inode_page_find_free(
        &self,
        mtr: &mut Mtr,
    ) -> Option<usize> {
        for n in 0..FSP_SEG_INODES_PER_PAGE {
            let inode = self.fsp_seg_inode_page_get_nth_inode(n, mtr);
            if inode.fseg_inode_get_fseg_id() == 0 {
                return Some(n);
            }
        }

        None
    }

    /// Find a used segment inode on a segment inode page.
    ///
    /// # Arguments
    ///
    /// * `mtr` - Mini-transaction for logging the operation
    ///
    /// # Returns
    ///
    /// `Some(n)` where `n` is the index of the first used inode, or `None` if all inodes are unused.
    ///
    pub fn fsp_seg_inode_page_find_used(
        &self,
        mtr: &mut Mtr,
    ) -> Option<usize> {
        for n in 0..FSP_SEG_INODES_PER_PAGE {
            let inode = self.fsp_seg_inode_page_get_nth_inode(n, mtr);
            if inode.fseg_inode_get_fseg_id() != 0 {
                return Some(n);
            }
        }

        None
    }

    /// Get all the segment ids on the fseg inode page.
    ///
    /// # Returns
    ///
    /// `Vec<FSegId>` containing all the segment ids on the page.
    ///
    #[cfg(test)]
    pub fn fsp_seg_inode_page_get_seg_ids(
        &self,
        mtr: &mut Mtr,
    ) -> Vec<FSegId> {
        let mut seg_ids = Vec::new();
        for n in 0..FSP_SEG_INODES_PER_PAGE {
            let inode = self.fsp_seg_inode_page_get_nth_inode(n, mtr);
            seg_ids.push(FSegId(inode.fseg_inode_get_fseg_id()));
        }
        seg_ids
    }
}
