/// The FSP (File Space) module handles the management of tablespace files in InnoDB.
///
/// This module is responsible for:
/// - Managing the allocation and deallocation of pages within tablespaces
/// - Tracking free and used extents (groups of pages)
/// - Maintaining the on-disk structures that organize tablespace data
///
/// The module consists of:
/// - `fsp0fsp`: Core implementation of file space management functions
/// - `fsp0types`: Type definitions and constants for file space management
pub(crate) mod fsp0fsp;
mod fsp0fsp_tests;
pub(crate) mod fsp0types;
