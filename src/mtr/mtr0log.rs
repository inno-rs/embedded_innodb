use crate::{
    innodb::innodb0types::{PageFrame, UNIV_PAGE_SIZE},
    mach::mach0data::{
        mach_uint64_write_compressed, mach_write_compressed, mach_write_to_1,
        mach_write_to_2, mach_write_to_4, mach_write_to_8,
    },
    mtr::{
        mtr0mtr::Mtr,
        mtr0types::{MlogType, MtrLogMode::MTR_LOG_NONE},
    },
    page::page0page::Page,
    ut::ut0dbg::ut_ad,
    ut_panic,
};

pub struct MLog {
    pub buf: Vec<u8>,
}

impl MLog {
    pub fn new() -> Self {
        MLog { buf: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }
    pub fn append(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Write 1-4 bytes to file page buffered in the buffer pool. And, write the corresponding log recording to the mini-transaction log.
    ///
    /// # Arguments
    ///
    /// * `page` - The page to write to.
    /// * `val` - The value to write.
    /// * `mlog_type` - The type of log to write.
    /// * `mtr` - The mini-transaction to write to.
    ///
    /// Write 1/2/4 bytes into the page at `offset`, and emit a complete
    /// redo-log record: [header][offset][value].
    pub fn mlog_write_u32(
        &mut self,
        page: &Page,
        offset: usize,
        val: u32,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        // 1) In-memory update
        let frame = page.to_page_frame();
        match mlog_type {
            MlogType::MLOG_1BYTE => {
                mach_write_to_1(&mut frame.0[offset..offset + 1], val as u8);
            }
            MlogType::MLOG_2BYTES => {
                mach_write_to_2(&mut frame.0[offset..offset + 2], val as u16);
            }
            MlogType::MLOG_4BYTES => {
                mach_write_to_4(&mut frame.0[offset..offset + 4], val as u32);
            }
            _ => ut_panic!("Invalid mlog type for u32 write: {:?}", mlog_type),
        }

        // 2) Bail if logging disabled
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }

        // 3) Reserve worst-case header (11) + 2B offset + 4B value
        const HEADER_SZ: usize = 11;
        const OFF_SZ: usize = 2;
        const VAL_SZ: usize = 4;
        let start = self.buf.len();
        self.buf.resize(start + HEADER_SZ + OFF_SZ + VAL_SZ, 0);

        // 4) Write the initial header: type + varints(space, page)
        let mut pos = self
            .mlog_write_initial_log_record_fast(page, mlog_type, start, mtr);

        // 5) Write the in-page offset (2-byte little-endian)
        let off_bytes = (offset as u16).to_le_bytes();
        self.buf[pos..pos + 2].copy_from_slice(&off_bytes);
        pos += 2;

        // 6) Write the 4-byte little-endian value
        let val_bytes = val.to_le_bytes();
        self.buf[pos..pos + 4].copy_from_slice(&val_bytes);
        pos += 4;

        // done! (mlog_close is a no-op)
    }

    /// Write an 8-byte LE value at `offset` in `page` and log it exactly like
    /// C++ `mlog_write_uint64` does in this fork.
    /// Exactly like C++ `mlog_write_uint64`, but using your PageFrame API
    pub fn mlog_write_uint64(
        &mut self,
        page: &Page,
        offset: usize,
        val: u64,
        mtr: &mut Mtr,
    ) {
        // 1) Get a safe mutable view of the page bytes
        let frame: PageFrame = page.to_page_frame();

        // 2) Apply the 8‐byte LE change into the frame
        mach_write_to_8(&mut frame.0[offset..offset + 8], val);

        // 3) If logging is off, bail out
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }

        // 4) Reserve space for header + offset + compressed payload
        const HEADER_SZ: usize = 11; // max header (1 + 5 + 5)
        const OFF_SZ: usize = 2; // 2 bytes for page-offset
        const MAX_U64: usize = 9; // max compressed u64 (5 + 4)
        let start = self.buf.len();
        self.buf.resize(start + HEADER_SZ + OFF_SZ + MAX_U64, 0);

        // 5) Write the initial header (type + varints)
        let mut pos = self.mlog_write_initial_log_record_fast(
            page,
            MlogType::MLOG_8BYTES,
            start,
            mtr,
        );

        // 6) Write the 2-byte page offset (little-endian)
        // This is a byte offset withing the page.
        let off_bytes = (offset as u16).to_le_bytes();
        self.buf[pos..pos + 2].copy_from_slice(&off_bytes);
        pos += 2;

        // 7) Write the compressed u64 (high varint + low 4 bytes)
        let written = mach_uint64_write_compressed(&mut self.buf[pos..], val);
        pos += written;
    }

    /// Copy `data` into `page` at `offset`, then emit a full
    /// [header][offset][len][data] redo‐record for MLOG_WRITE_STRING.
    pub fn mlog_write_string(
        &mut self,
        page: &Page,
        offset: usize,
        data: &[u8],
        mtr: &mut Mtr,
    ) {
        // 1) In‐memory update
        let frame = page.to_page_frame();
        frame.0[offset..offset + data.len()].copy_from_slice(data);

        // 2) Bail if logging is disabled
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }

        // 3) Reserve space: 11-byte worst-case header + 2B offset + 2B len + payload
        const HEADER_SZ: usize = 11;
        let start = self.buf.len();
        let reserve = HEADER_SZ + 2 + 2 + data.len();
        self.buf.resize(start + reserve, 0);

        // 4) Emit the “fast” header: 1B type + varint(space_id) + varint(page_no)
        let mut pos = self.mlog_write_initial_log_record_fast(
            page,
            MlogType::MLOG_WRITE_STRING,
            start,
            mtr,
        );

        // 5) Write 2-byte little-endian page‐offset
        let off_bytes = (offset as u16).to_le_bytes();
        self.buf[pos..pos + 2].copy_from_slice(&off_bytes);
        pos += 2;

        // 6) Write 2-byte little-endian length
        let len_bytes = (data.len() as u16).to_le_bytes();
        self.buf[pos..pos + 2].copy_from_slice(&len_bytes);
        pos += 2;

        // 7) Copy the actual string bytes
        self.buf[pos..pos + data.len()].copy_from_slice(data);
    }

    /// Emit the end-of-sequence marker for multi-record writes.
    pub fn mlog_multi_rec_end(&mut self, mtr: &mut Mtr) {
        self.buf.push(MlogType::MLOG_MULTI_REC_END as u8);
        mtr.inc_n_log_records();
    }

    /// Append (concatenate) a string into the MTR log, if logging is enabled.
    pub fn mlog_catenate_string(
        &mut self,
        page: &[u8],
        offset: usize,
        len: usize,
        mtr: &Mtr,
    ) {
        // 1) If logging is disabled, do nothing
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }
        // 2) Append exactly those bytes
        self.buf.extend_from_slice(&page[offset..offset + len]);
    }
    /// Write the “fast” redo-record header:
    /// [1B type][varint space_id][varint page_no]
    /// Returns the buffer index immediately after the header.
    pub fn mlog_write_initial_log_record_fast(
        &mut self,
        page: &Page,
        mlog_type: MlogType,
        mut log_pos: usize,
        mtr: &mut Mtr,
    ) -> usize {
        // --- PREPARE SPACE UP FRONT ---
        // Worst-case header length = 1 + 5 + 5 = 11 bytes
        const MAX_HDR: usize = 11;
        let needed = log_pos + MAX_HDR;
        if self.buf.len() < needed {
            self.buf.resize(needed, 0);
        }
        // Ensure we won't reallocate below:
        self.buf.reserve(0);

        // --- 1) RECORD TYPE ---
        self.buf[log_pos] = mlog_type as u8;
        log_pos += 1;

        // --- 2) COMPRESS & WRITE space_id ---
        let space = page.page_get_space_id();
        let n_space = mach_write_compressed(&mut self.buf[log_pos..], space);
        log_pos += n_space;

        // --- 3) COMPRESS & WRITE page_no ---
        let pageno = page.page_get_fil_offset();
        let n_page =
            mach_write_compressed(&mut self.buf[log_pos..], pageno as u32);
        log_pos += n_page;

        // --- 4) BUMP RECORD COUNT ---
        mtr.inc_n_log_records();

        // Return where the caller can append payload
        log_pos
    }

    pub fn mlog_write_initial_log_record(
        &mut self,
        page: &Page,
        mlog_type: MlogType,
        mtr: &mut Mtr,
    ) {
        // 1) Sanity checks: type in (8‐byte, BIGGEST] range
        let t = mlog_type as u8;
        ut_ad!(t <= MlogType::MLOG_BIGGEST_TYPE as u8);
        ut_ad!(t > MlogType::MLOG_8BYTES as u8);

        // 2) “mlog_open(mtr, 11)” → skip entirely if logging‐mode is NONE
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return;
        }

        // 3) If not in the dblwr‐create phase, emit the fast header
        if !mtr.dblwr_create_in_progress {
            // Reserve space for the worst‐case 11‐byte header
            let start = self.buf.len();
            const MAX_HEADER: usize = 11;
            if self.buf.len() < start + MAX_HEADER {
                self.buf.resize(start + MAX_HEADER, 0);
            }
            // Write it (advancing past those bytes)
            let _after = self.mlog_write_initial_log_record_fast(
                page, mlog_type, start, mtr,
            );
        }
    }

    /// Attempt to parse an MLOG_WRITE_STRING record from `input`.
    ///
    /// - `input` is the bytes remaining in the log (must contain at least 4 bytes for
    ///   offset+length, plus the payload).
    /// - `page` is an optional mutable page buffer; if `Some`, we write the payload
    ///   into `page[offset..offset+len]`. In plain terms: by making page an Option,
    /// we give callers a choice—either just parse and skip the memory write (None),
    /// or parse and apply the change (Some(page))—all with one shared function rather than two separate ones.
    ///
    /// Returns `Some(remaining)` on success, where `remaining` is the slice after
    /// consuming the 4 + len bytes. Returns `None` on any underflow or bounds failure.
    pub fn mlog_parse_string<'a>(
        &mut self,
        mut input: &'a [u8],
        page: Option<&mut [u8]>,
    ) -> Option<&'a [u8]> {
        // 1) Need at least 4 bytes: 2 for offset, 2 for length
        if input.len() < 4 {
            return None;
        }

        // 2) Read offset + length (little-endian)
        let offset = u16::from_le_bytes([input[0], input[1]]) as usize;
        let len = u16::from_le_bytes([input[2], input[3]]) as usize;
        input = &input[4..];

        // 3) Bounds check against page size
        if offset >= UNIV_PAGE_SIZE || offset + len > UNIV_PAGE_SIZE {
            // In C++ they'd flag a corrupt-log error here
            return None;
        }

        // 4) Need at least `len` bytes for the payload
        if input.len() < len {
            return None;
        }

        // 5) If a page buffer was provided, copy the payload into it

        if let Some(page_buf) = page {
            page_buf[offset..offset + len].copy_from_slice(&input[..len]);
        }

        // 6) Consume the payload and return the rest
        Some(&input[len..])
    }

    /// Reserve log‐space for an index‐write (11 + `size` bytes),
    /// emit the 1‐byte header, and return a mutable slice of length `size`
    /// where the caller can write its payload.  Returns `None` if logging disabled.
    ///
    ///
    /// Imagine you’re inserting a new secondary‐index entry for customer ID 123:
    ///1.	Compute your payload: maybe it’s the 16-byte B-tree entry holding the key + record pointer.
    /// 2.	Call mlog_open_and_write_index(mtr, ptr, MLOG_INDEX_INSERT, 16).
    /// 	If logging is off, it returns nullptr and you skip all this.
    ///	Otherwise it grows the log buffer by 11 + 16 bytes, writes the 1-byte type and reserves room for the varints.
    /// 3.	Fill in the 16-byte payload at the returned pointer.
    ///
    ///
    pub fn mlog_open_and_write_index(
        &mut self,
        mtr: &mut Mtr,
        mlog_type: MlogType,
        size: usize,
    ) -> Option<&mut [u8]> {
        // Mirrors mlog_open(mtr, 11 + size):
        // If no logging, bail out.
        if mtr.get_log_mode() == MTR_LOG_NONE {
            return None;
        }

        // Reserve exactly 11 + size bytes in the buffer.
        // We remember the starting offset so we can slice out the payload region.
        const HEADER_LEN: usize = 11;
        let start = self.buf.len();
        self.buf.resize(start + HEADER_LEN + size, 0);

        // Emit the “fast” initial header at `start` (1 byte + compressed page info).
        // Here we only push the record‐type byte and bump the log‐record count;
        // your `mlog_write_initial_log_record_fast` may encode more meta.
        mach_write_to_1(&mut self.buf[start..start + 1], mlog_type as u8);
        mtr.inc_n_log_records();

        // Compute where the caller’s payload begins:
        let payload_start = start + HEADER_LEN;
        let payload_end = payload_start + size;

        // In C++ they do:
        //   if size==0 close immediately; else if overflow reopen…
        // You can mimic that logic here if needed.  For now we simply hand back
        // the slice for writing, or `None` if size==0.
        if size == 0 {
            // no payload to fill, so treat like “closed”:
            return None;
        }

        Some(&mut self.buf[payload_start..payload_end])
    }
}
