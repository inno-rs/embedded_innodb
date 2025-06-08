use crate::block_on;
/// This module provides file I/O operations for the embedded InnoDB storage engine.
/// It includes functionality for file creation, opening, reading, writing, and size retrieval.
use crate::innodb::innodb0types::IB_FILE_BLOCK_SIZE;
use crate::ut::ut0dbg::ut_ad;
use crate::ut::ut0ut::GLOBAL_RUNTIME;
use std::cmp::min;
use std::io::SeekFrom;
use strum_macros::Display;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
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
#[derive(Display, PartialEq, Clone, Debug)]
pub enum IORequest {
    /// No I/O operation.
    None,
    /// Asynchronous read operation.
    AsyncRead,
    /// Asynchronous write operation.
    AsyncWrite,
    /// Asynchronous log read operation.
    AsyncLogRead,
    /// Asynchronous log write operation.
    AsyncLogWrite,
    /// Synchronous read operation.
    SyncRead,
    /// Synchronous write operation.
    SyncWrite,
    /// Synchronous log read operation.
    SyncLogRead,
    /// Synchronous log write operation.
    SyncLogWrite,
}

impl IORequest {
    /// Checks if the I/O request is a read operation.
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            IORequest::AsyncRead
                | IORequest::SyncRead
                | IORequest::AsyncLogRead
                | IORequest::SyncLogRead
        )
    }

    /// Checks if the I/O request is a write operation.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            IORequest::AsyncWrite
                | IORequest::SyncWrite
                | IORequest::AsyncLogWrite
                | IORequest::SyncLogWrite
        )
    }

    /// Checks if the I/O request is a log read operation.
    ///
    /// # Returns
    ///
    /// * `true` if the request is either AsyncLogRead or SyncLogRead.
    /// * `false` otherwise.
    pub fn is_log_read(&self) -> bool {
        matches!(self, IORequest::AsyncLogRead | IORequest::SyncLogRead)
    }

    /// Checks if the I/O request is a log write operation.
    ///
    /// # Returns
    ///
    /// * `true` if the request is either AsyncLogWrite or SyncLogWrite.
    /// * `false` otherwise.
    pub fn is_log_write(&self) -> bool {
        matches!(self, IORequest::AsyncLogWrite | IORequest::SyncLogWrite)
    }

    /// Checks if the I/O request is synchronous.
    pub fn is_sync(&self) -> bool {
        matches!(
            self,
            IORequest::SyncRead
                | IORequest::SyncWrite
                | IORequest::SyncLogRead
                | IORequest::SyncLogWrite
        )
    }
}

/// Contains the result of a read operation.
///
/// This struct is returned when a read operation completes successfully.
/// It can be extended in the future to include additional information about the read operation,
/// such as the number of bytes read or performance metrics.
pub struct ReadResult();

/// Contains the result of a write operation.
///
/// This struct is returned when a write operation completes successfully.
/// It can be extended in the future to include additional information about the write operation,
/// such as the number of bytes written or performance metrics.
/// The struct implements Eq, PartialEq, and Debug traits for comparison and debugging purposes.
#[derive(Eq, PartialEq, Debug)]
pub struct WriteResult();

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
            let fil_fut = open_options
                .create(true)
                .write(true)
                .read(true)
                .custom_flags(libc::O_DSYNC)
                .open(name);
            Some(Runtime::new().unwrap().block_on(fil_fut).unwrap())
        }
        CreateMode::OsFileCreate => {
            let fil_fut = open_options
                .create_new(true)
                .write(true)
                .read(true)
                .custom_flags(libc::O_DSYNC)
                .open(name);
            match Runtime::new().unwrap().block_on(fil_fut) {
                Ok(file) => Some(file),
                Err(_) => None,
            }
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

/// Writes data to a file at the specified offset.
///
/// # Arguments
///
/// * `os_file` - A mutable reference to the file to write to
/// * `data` - The data to write to the file
/// * `offset` - The offset in the file where writing should begin
/// * `len` - The number of bytes to write
///
/// # Returns
///
/// * `Ok(WriteResult)` - If the write operation was successful
/// * `Err(String)` - If an error occurred during the write operation, with an error message
pub async fn os_file_write_async(
    os_file: &mut OsFile,
    data: &[u8],
    offset: usize,
    len: usize,
) -> Result<WriteResult, String> {
    ut_ad!(offset % IB_FILE_BLOCK_SIZE == 0);
    ut_ad!(len % IB_FILE_BLOCK_SIZE == 0);

    if let Err(err) = os_file.seek(SeekFrom::Start(offset as u64)).await
    {
        return Err(format!("Failed to seek in file: {}", err));
    }

    if let Err(err) = os_file.write(data).await
    {
        return Err(format!("Failed to write to file: {}", err));
    }

    Ok(WriteResult {})
}

/// Writes data to a file at the specified offset.
///
/// This is a synchronous wrapper around the asynchronous `os_file_write_async` function.
///
/// # Arguments
///
/// * `os_file` - The file to write to
/// * `data` - The data to write
/// * `offset` - The offset in the file to start writing at
/// * `len` - The number of bytes to write
///
/// # Returns
///
/// * `Ok(WriteResult)` - If the write operation was successful
/// * `Err(String)` - If an error occurred during the write operation, with an error message
pub fn os_file_write(
    os_file: &mut OsFile,
    data: &[u8],
    offset: usize,
    len: usize,
) -> Result<WriteResult, String> {
    GLOBAL_RUNTIME.block_on(os_file_write_async(
        os_file,
        data,
        offset,
        len,
    ))
}

/// Reads data from a file at the specified offset.
///
/// # Arguments
///
/// * `os_file` - A mutable reference to the file to read from
/// * `offset` - The offset in the file where reading should begin
/// * `len` - The number of bytes to read
/// * `buffer` - A mutable slice where the read data will be stored
///
/// # Returns
///
/// * `Ok(ReadResult)` - If the read operation was successful, containing the read data
/// * `Err(String)` - If an error occurred during the read operation, with an error message
pub async fn os_file_read_async(
    os_file: &mut OsFile,
    offset: usize,
    len: usize,
    buffer: &mut [u8],
) -> Result<ReadResult, String> {
    ut_ad!(offset % IB_FILE_BLOCK_SIZE == 0);
    ut_ad!(len % IB_FILE_BLOCK_SIZE == 0);

    if let Err(err) = os_file.seek(SeekFrom::Start(offset as u64)).await {
        return Err(format!("Failed to seek in file: {}", err));
    };

    if let Err(err) = os_file.read(buffer).await {
        return Err(format!("Failed to read from file: {}", err));
    }

    Ok(ReadResult())
}

/// Reads data from a file at the specified offset.
///
/// This is a synchronous wrapper around the asynchronous `os_file_read_async` function.
///
/// # Arguments
///
/// * `os_file` - The file to read from
/// * `offset` - The offset in the file to start reading from
/// * `len` - The number of bytes to read
/// * `buffer` - The buffer to read the data into
///
/// # Returns
///
/// * `Ok(ReadResult)` - If the read operation was successful
/// * `Err(String)` - If an error occurred during the read operation, with an error message
pub fn os_file_read(
    os_file: &mut OsFile,
    offset: usize,
    len: usize,
    buffer: &mut [u8],
) -> Result<ReadResult, String> {
    GLOBAL_RUNTIME.block_on(os_file_read_async(
        os_file,
        offset,
        len,
        buffer,
    ))
}


/// Asynchronously sets the size of a file.
///
/// This function will either extend or truncate the file to the specified size.
/// If the file needs to be extended, it will be filled with zeros.
///
/// # Arguments
///
/// * `os_file` - A mutable reference to the file whose size is to be set
/// * `size` - The new size of the file in bytes
///
/// # Returns
///
/// * `Ok(())` - If the operation was successful
/// * `Err(String)` - If an error occurred during the operation, with an error message
pub async fn os_file_set_size_async(
    os_file: &mut OsFile,
    size: u64,
) -> Result<(), String> {
    let current_len = os_file
        .metadata()
        .await
        .map_err(|e| format!("Failed to get metadata: {}", e))?
        .len();
    let new_len = size as u64;

    if new_len > current_len {
        // Seek to the current end of the file.
        os_file
            .seek(SeekFrom::Start(current_len))
            .await
            .map_err(|e| format!("Failed to seek: {}", e))?;

        // Write zeroes to the new area
        const CHUNK_SIZE: usize = IB_FILE_BLOCK_SIZE;
        let zeros = [0u8; CHUNK_SIZE];
        let mut remaining = new_len - current_len;

        while remaining > 0 {
            let write_size = min(CHUNK_SIZE as u64, remaining) as usize;
            os_file
                .write_all(&zeros[..write_size])
                .await
                .map_err(|e| format!("Failed to write zeros: {}", e))?;
            remaining -= write_size as u64;
        }
    }

    // Finalize with set_len to ensure the correct size (in case of race).
    os_file
        .set_len(new_len)
        .await
        .map_err(|e| format!("Failed to set length: {}", e))?;

    Ok(())
}

/// Sets the size of a file synchronously.
///
/// This is a synchronous wrapper around `os_file_set_size_async` that blocks the current thread
/// until the operation completes.
///
/// # Arguments
///
/// * `os_file` - A mutable reference to the file whose size is to be set
/// * `size` - The new size of the file in bytes
///
/// # Returns
///
/// * `Ok(())` - If the operation was successful
/// * `Err(String)` - If an error occurred during the operation, with an error message
pub fn os_file_set_size(
    os_file: &mut OsFile,
    size: u64,
) -> Result<(), String> {
    GLOBAL_RUNTIME.block_on(os_file_set_size_async(os_file, size))
}

/// Asynchronously removes a file from the file system.
///
/// # Arguments
///
/// * `name` - The path to the file to be removed
///
/// # Returns
///
/// * `Ok(())` - If the file was successfully removed
/// * `Err(String)` - If an error occurred during the operation, with an error message
pub async fn os_file_delete_async(name: &str) -> Result<(), String> {
    tokio::fs::remove_file(name)
        .await
        .map_err(|e| format!("Failed to remove file '{}': {}", name, e))
}

/// Synchronously removes a file from the file system.
///
/// This is a synchronous wrapper around `os_remove_file_async` that uses the global runtime
/// to block the current thread until the operation completes.
///
/// # Arguments
///
/// * `name` - The path to the file to be removed
///
/// # Returns
///
/// * `Ok(())` - If the file was successfully removed
/// * `Err(String)` - If an error occurred during the operation, with an error message
pub fn os_file_delete(name: &str) -> Result<(), String> {
    GLOBAL_RUNTIME.block_on(os_file_delete_async(name))
}
