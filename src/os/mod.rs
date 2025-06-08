/// Operating system abstraction layer for the embedded InnoDB storage engine.
///
/// This module provides interfaces for file I/O operations, including file creation,
/// opening, reading, writing, and other OS-level functionality required by the database.
/// It abstracts away platform-specific details to provide a consistent interface
/// across different operating systems.
pub(crate) mod os0file;
mod os0file_tests;
pub(crate) mod os0io;
pub(crate) mod os0runtime;
