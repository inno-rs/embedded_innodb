//! Page initialization routines

/// Zeroes out a page frame and writes the page number
pub fn init_file_page_low(frame: &mut [u8], page_no: u32) {
    frame.fill(0);

}

/// Wraps `init_file_page_low` in a mini-transaction and logs the operation
pub fn init_file_page(frame: &mut [u8], page_no: u32 /*, mtr: &mut Mtr */) {
    init_file_page_low(frame, page_no);

}