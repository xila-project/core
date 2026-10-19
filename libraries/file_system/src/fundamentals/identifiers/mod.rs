//! Identifier types for file system objects and operations.
//!
//! This module provides various identifier types used throughout the file system
//! to uniquely identify files, directories, file systems, inodes, and other objects.
//! These identifiers ensure type safety and provide a consistent way to reference
//! file system entities across different operations.

mod entry;

pub use entry::*;

pub type Inode = u64;
