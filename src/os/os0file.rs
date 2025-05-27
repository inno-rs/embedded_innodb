use crate::innodb::innodb0types::IB_FILE_BLOCK_SIZE;
use crate::ut::ut0dbg::ut_ad;
use std::io::SeekFrom;
/// This module provides file I/O operations for the embedded InnoDB storage engine.
/// It includes functionality for file creation, opening, reading, writing, and size retrieval.
use strum_macros::Display;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWriteExt};
use tokio::runtime::Runtime;

/// Represents a file handle in the operating system.
///
/// On Linux systems, this is a direct alias to tokio's File type.
#[cfg(target_os = "linux")]
pub type OsFile = File;

/// Represents a file handle in the operating system.
///
/// On non-Linux systems, this is also an alias to tokio's File type.
#[cfg(not(target_os = "linux"))]
pub type OsFile = File;

/// Represents the type of I/O operation to be performed.
///
/// This enum is used to specify whether an I/O operation should be
/// synchronous or asynchronous, and whether it's a read or write operation.
#[derive(Display, PartialEq)]
pub enum IORequest {
    /// No I/O operation.
    None,
    /// Asynchronous read operation.
    AsyncRead,
    /// Asynchronous write operation.
    AsyncWrite,
    /// Synchronous read operation.
    SyncRead,
    /// Synchronous write operation.
    SyncWrite,
}

/// Contains the result of a read operation.
pub struct ReadResult {
    /// Pointer to the buffer containing the read data.
    pub data: Vec<u8>,
}

/// Contains the result of a write operation.
pub struct WriteResult {}

/// Represents the result of an I/O operation.
///
/// This enum can contain either a read result or a write result,
/// depending on the type of operation that was performed.
pub enum IOResult {
    /// Result of a read operation.
    Read(ReadResult),
    /// Result of a write operation.
    Write(WriteResult),
}

/// Specifies the mode for creating or opening a file.
pub enum CreateMode {
    /// Open an existing file. If the file doesn't exist, create it.
    OsFileOpen,
    /// Create a new file. If the file already exists, the operation fails.
    OsFileCreate,
    /// Create a new file or overwrite an existing one.
    OsFileOverwrite,
    /// Open a file in raw mode, bypassing the operating system's cache.
    OsFileRaw,
    /// Create a file and any necessary parent directories.
    OsFileCreatePath,
}

/// Opens an existing file or creates a new one.
///
/// # Arguments
///
/// * `name` - The path to the file to open or create
/// * `create_mode` - The mode to use when opening or creating the file
///
/// # Returns
///
/// * `Some(OsFile)` - If the file was successfully opened or created
/// * `None` - If the operation failed or the create_mode is not supported
pub fn os_file_create(name: &str, create_mode: CreateMode) -> Option<OsFile> {
    let mut open_options = OpenOptions::new();
    match create_mode {
        CreateMode::OsFileOpen => {
            let fil_fut = open_options.create(true).write(true).open(name);
            Some(Runtime::new().unwrap().block_on(fil_fut).unwrap())
        }
        _ => None,
    }
}

/// Gets the size of a file in bytes.
///
/// # Arguments
///
/// * `os_file` - A reference to the file whose size is to be determined
///
/// # Returns
///
/// * The size of the file in bytes if successful
/// * -1 if an error occurred while getting the file size
pub fn os_file_get_size(os_file: &OsFile) -> i64 {
    let rt = Runtime::new().unwrap();
    let metadata = os_file.metadata();
    match rt.block_on(metadata) {
        Ok(meta) => meta.len() as i64,
        Err(_) => -1,
    }
}

pub fn os_file_write(
    os_file: &mut OsFile,
    data: &[u8],
    offset: usize,
    len: usize,
) -> Result<WriteResult, String> {
    ut_ad!(offset % IB_FILE_BLOCK_SIZE == 0);
    ut_ad!(len % IB_FILE_BLOCK_SIZE == 0);

    let rt = Runtime::new().unwrap();
    let seek_fut = os_file.seek(SeekFrom::Start(offset as u64));
    match rt.block_on(seek_fut) {
        Ok(_) => {
            let write_fut = os_file.write(&data[..len]);
            let res = rt.block_on(write_fut).unwrap();
            Ok(WriteResult {})
        }
        Err(err) => Err(format!("Failed to seek to file: {}", err)),
    }
}

pub fn os_file_read(
    os_file: &mut OsFile,
    offset: usize,
    len: usize,
) -> Result<ReadResult, String> {
    ut_ad!(offset % IB_FILE_BLOCK_SIZE == 0);
    ut_ad!(len % IB_FILE_BLOCK_SIZE == 0);

    let rt = Runtime::new().unwrap();
    let seek_fut = os_file.seek(SeekFrom::Start(offset as u64));
    match rt.block_on(seek_fut) {
        Ok(_) => {
            let mut buffer = vec![0; len];
            let read_fut = os_file.read(&mut buffer);
            match rt.block_on(read_fut) {
                Ok(_) => Ok(ReadResult { data: buffer }),
                Err(err) => Err(format!("Failed to read from file: {}", err)),
            }
        }
        Err(err) => Err(format!("Failed to seek in file: {}", err)),
    }
}
