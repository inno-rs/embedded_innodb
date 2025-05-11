use tokio::fs::{File, OpenOptions};
use tokio::runtime::Runtime;

#[cfg(target_os = "linux")]
pub type OsFile = File;

#[cfg(not(target_os = "linux"))]
pub type OsFile = File;

/// Opens an existing file or creates a new one.
pub fn os_file_create(name: &str) -> OsFile {
    let mut open_options = OpenOptions::new();
    let fil_fut = open_options.create(true).write(true).open(name);
    Runtime::new().unwrap().block_on(fil_fut).unwrap()
}
