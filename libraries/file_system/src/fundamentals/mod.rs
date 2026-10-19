//! Fundamental types and structures for file system operations.
//!
//! This module contains the core data types used throughout the file system,
//! including paths, sizes, positions, metadata, permissions, and other essential
//! building blocks for file system operations.

mod attributes;
mod control;
mod entry;
mod flags;
mod identifiers;
mod kind;
mod path;
mod permission;
mod position;
mod size;
mod statistics;

pub use attributes::*;
pub use control::*;
pub use entry::*;
pub use flags::*;
pub use identifiers::*;
pub use kind::*;
pub use path::*;
pub use permission::*;
pub use position::*;
pub use size::*;
pub use statistics::*;

/// Standard block size representation for file system operations.
///
/// This type represents a 512-byte block, which is the standard sector size
/// for most storage devices. It's used throughout the file system for
/// block-aligned operations and buffer management.
///
/// # Examples
///
/// ```rust
/// # extern crate alloc;
/// use file_system::Block;
///
/// let block = Block::default();
/// assert_eq!(block.0.len(), 512);
/// ```
#[repr(transparent)]
pub struct Block(pub [u8; 512]);

impl Default for Block {
    fn default() -> Self {
        Block([0; 512])
    }
}
