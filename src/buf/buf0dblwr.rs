use crate::buf::buf0types::BufBlock;
use crate::fsp::fspOfsp::Fsp;
use crate::innodb::innodb0types::{PageNo, SpaceId, UNIV_PAGE_SIZE};
use crate::some_or_panic;
use crate::ut::ut0dbg::{ut_a, ut_ad};
use std::ops::DerefMut;
use std::sync::{Arc, RwLock};

/// Size of the double write buffer.
/// The formula is 1 + 2 * 128 * UNIV_PAGE_SIZE, which allocates enough space
/// for the buffer header and two segments of 128 pages each.
pub const DOUBLE_WRITE_BUFFER_SIZE: usize = 1 + 2 * UNIV_PAGE_SIZE;

/// A wrapper around a raw byte array that holds the double write buffer data.
/// This is used to ensure proper memory alignment for database pages.
#[repr(C, align(16384/* UNIV_PAGE_SIZE*/))]
pub struct Block(pub [u8; DOUBLE_WRITE_BUFFER_SIZE]);

/// The main double write buffer implementation.
///
/// The double write buffer is a critical component in InnoDB for crash recovery.
/// It writes pages twice: first to the double write buffer and then to the actual
/// data files. This ensures that even if a page write is interrupted (e.g., due to
/// a power failure), the database can recover using the copy in the double write buffer.
pub struct DoubleWriteBuffer {
    /// Reference to the file space system
    fsp: Arc<RwLock<Fsp>>,
    /// The actual buffer storage
    block: Box<Block>,
    /// Index of the first free slot in the buffer
    first_free: usize,
    /// Collection of pages currently in the buffer
    pages: Vec<DoubleWriteBufferPage>,
}

/// Represents a page in the double write buffer.
///
/// This struct tracks the space ID and page number of a database page
/// that has been written to the double write buffer, along with its
/// position (slot) within the buffer.
#[derive(Clone)]
pub struct DoubleWriteBufferPage {
    /// The space ID of the page
    pub space_id: SpaceId,
    /// The page number within its space
    pub page_no: PageNo,
    /// The slot ID in the double write buffer where this page is stored
    slot_id: usize,
}

impl DoubleWriteBufferPage {
    /// Creates a new DoubleWriteBufferPage with the specified space ID and page number.
    ///
    /// # Parameters
    /// * `space_id` - The space ID of the page
    /// * `page_no` - The page number within its space
    ///
    /// # Returns
    /// A new DoubleWriteBufferPage with the slot_id initialized to usize::MAX
    pub fn new(space_id: SpaceId, page_no: PageNo) -> Self {
        Self { space_id, page_no, slot_id: usize::MAX }
    }

    /// Creates a dummy DoubleWriteBufferPage with maximum values for space_id and page_no.
    ///
    /// This is typically used for initializing the buffer with placeholder pages
    /// that will be replaced with actual pages later.
    ///
    /// # Returns
    /// A DoubleWriteBufferPage with maximum values for space_id, page_no, and slot_id
    pub fn dummy() -> Self {
        Self {
            space_id: SpaceId::MAX,
            page_no: PageNo::MAX,
            slot_id: usize::MAX,
        }
    }
}

impl DoubleWriteBuffer {
    /// Creates a new DoubleWriteBuffer with the specified file space, block, and size.
    ///
    /// This constructor ensures that the provided block is properly aligned to page boundaries,
    /// which is critical for database page operations.
    ///
    /// # Parameters
    /// * `fsp` - Reference to the file space system
    /// * `block` - The pre-allocated memory block for the double write buffer
    /// * `size` - The number of pages the buffer can hold
    ///
    /// # Returns
    /// A new DoubleWriteBuffer instance
    ///
    /// # Panics
    /// Panics if the provided block is not properly aligned to page boundaries
    pub fn new(fsp: Arc<RwLock<Fsp>>, block: Box<Block>, size: usize) -> Self {
        let ptr = block.as_ref() as *const Block as *const u8;
        // Ensure the block is properly aligned
        ut_a!(
            ptr as usize % UNIV_PAGE_SIZE == 0,
            "DoubleWriteBuffer block is not properly aligned"
        );

        Self {
            fsp,
            block,
            first_free: 0,
            pages: vec![DoubleWriteBufferPage::dummy(); size],
        }
    }

    /// Retrieves a raw byte slice for a specific page in the double write buffer.
    ///
    /// # Parameters
    /// * `dblwr_buffer_page` - The page descriptor for which to retrieve the raw block
    ///
    /// # Returns
    /// * `Some(&mut [u8])` - A mutable slice of bytes representing the page if it exists in the buffer
    /// * `None` - If the page is not in the buffer or the slot_id is invalid
    pub fn get_raw_block(
        &mut self,
        dblwr_buffer_page: &DoubleWriteBufferPage,
    ) -> Option<&mut [u8]> {
        if dblwr_buffer_page.slot_id >= self.first_free {
            return None;
        }

        Some(
            &mut self.block.0[dblwr_buffer_page.slot_id * UNIV_PAGE_SIZE
                ..(dblwr_buffer_page.slot_id + 1) * UNIV_PAGE_SIZE],
        )
    }

    /// Checks if the double write buffer is empty.
    ///
    /// # Returns
    /// `true` if the buffer is empty, `false` otherwise
    pub fn is_empty(&self) -> bool {
        self.first_free == 0
    }

    /// Resets the double write buffer to empty state.
    ///
    /// This method marks the buffer as empty by resetting the first_free index,
    /// but does not actually clear the data in the buffer.
    pub fn set_empty(&mut self) {
        self.first_free = 0;
    }

    /// Retrieves a list of all modified pages in the double write buffer.
    ///
    /// This method collects all pages that have been added to the buffer
    /// and returns them as a vector.
    ///
    /// # Returns
    /// A vector of DoubleWriteBufferPage objects representing the modified pages
    ///
    /// # Panic
    /// Panics if:
    /// - A page cannot be retrieved from the buffer
    /// - A page's slot_id is not set to usize::MAX
    pub fn get_modified_blocks(&self) -> Vec<DoubleWriteBufferPage> {
        (0..self.first_free)
            .into_iter()
            .map(|i| {
                let page = some_or_panic!(
                    self.pages.get(i),
                    "failed to get page from DoubleWriteBuffer"
                );

                ut_a!(
                    page.slot_id == usize::MAX,
                    "DoubleWriteBufferPage slot_id is not set"
                );

                page.clone()
            })
            .collect::<Vec<DoubleWriteBufferPage>>()
    }

    /// Appends a buffer block to the double write buffer.
    ///
    /// This method copies the content of the provided buffer block to the next
    /// available slot in the double write buffer and updates the metadata
    /// (space_id and page_no) accordingly.
    ///
    /// # Parameters
    /// * `buf_block` - The buffer block to append to the double write buffer
    ///
    /// # Panic
    /// Panics if:
    /// - There is no space left in the double write buffer
    /// - The page cannot be retrieved from the buffer
    pub fn append(&mut self, buf_block: &mut BufBlock) -> Result<(), String> {
        let chunk = self
            .block
            .deref_mut()
            .0
            .chunks_mut(UNIV_PAGE_SIZE)
            .skip(self.first_free - 1)
            .next();

        ut_ad!(chunk.is_some(), "No space left in DoubleWriteBuffer");
        let chunk = chunk.unwrap();

        let page = buf_block.get_page().page_get_as_mut_slice();

        chunk.copy_from_slice(unsafe { &*page });
        let dblwr_buffer = some_or_panic!(
            self.pages.get_mut(self.first_free),
            "failed to get page from DoubleWriteBuffer"
        );

        let buf_page = buf_block.buf_block_get_page();
        let buf_page_ref = buf_page.borrow();

        dblwr_buffer.page_no = buf_page_ref.get_page_no();
        dblwr_buffer.space_id = buf_page_ref.get_space_id();

        self.first_free += 1;

        Ok(())
    }
}
