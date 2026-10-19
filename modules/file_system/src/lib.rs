//! # File System Module
//!
//! This crate provides a comprehensive file system abstraction layer for the Xila operating system.
//! It includes support for various file system operations, device management, partition handling,
//! and Master Boot Record (MBR) operations.
//!
//! ## Overview
//!
//! The File System module is designed to provide a unified interface for:
//! - File and directory operations (create, read, write, delete, etc.)
//! - Device abstraction for block devices and memory devices
//! - Partition management and MBR (Master Boot Record) support
//! - File system metadata handling (permissions, timestamps, etc.)
//! - Cross-platform file system traits
//!
//! ## Key Components
//!
//! ### File System Traits
//! - [`FileSystemOperations`] - Core trait for implementing file systems
//! - Support for POSIX-like operations with task and user isolation
//!
//! ### Device Management
//! - [`CharacterDevice`] - Character device abstraction
//! - [`BlockDevice`] - Block device abstraction
//! - [`DirectCharacterDevice`] - Context-free direct character device operations
//! - [`DirectBlockDevice`] - Context-free direct block device operations
//! - [`MemoryDevice`] - In-memory device implementation for testing
//!
//! ### Partition Support
//! - [`PartitionDevice`] - Device representing a partition on a larger device
//! - [`mbr::PartitionEntry`] - MBR partition table entry
//! - [`mbr::PartitionKind`] - Enumeration of partition types
//!
//! ### MBR (Master Boot Record)
//! - [`mbr::Mbr`] - Complete MBR structure with partition table
//! - Utilities for creating, reading, and validating MBRs
//! - Support for creating partition devices from MBR entries
//!
//! ### Fundamental Types
//! - [`Path`] - File system path representation
//! - [`Error`] - File system error enumeration
//! - [`Size`] - Size and position types
//! - [`Time`] - Timestamp handling
//! - [`Flags`] - File operation flags
//!
//! ## Features
//!
//! - `std` - Enables standard library support for environments that have it
//! - Default is `no_std` for embedded systems
//!
//! ## Examples
//!
//! ### Basic Device Operations
//!
//! ```rust
//! # extern crate alloc;
//! # use file_system::{MemoryDevice, DirectBaseOperations};
//!
//! // Create an in-memory device for testing
//! let device = MemoryDevice::<512>::new(1024 * 1024);
//!
//! // Write some data
//! let data = b"Hello, File System!";
//! let result = device.write(data, 0);
//! assert!(result.is_ok());
//! ```
//!
//! ### MBR Operations
//!
//! ```rust
//! extern crate alloc;
//! use file_system::{mbr::{Mbr, PartitionKind, create_partition_device}, MemoryDevice};
//!
//! // Create a device and format it with MBR
//! let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
//!
//! // Create MBR and add a partition
//! let mut mbr = Mbr::new_with_signature(0x12345678);
//! mbr.add_partition(PartitionKind::Fat32Lba, 2048, 8192, true).unwrap();
//!
//! // Write MBR to device
//! mbr.write_to_device(&device).unwrap();
//!
//! // Create a partition device
//! let partition = create_partition_device(&device, &mbr.partitions[0]).unwrap();
//! ```
//!
//! ## Safety and Concurrency
//!
//! This crate is designed to be thread-safe and supports concurrent access to file systems
//! and devices. All device implementations must be `Send + Sync` and should use appropriate
//! synchronization primitives to handle concurrent access.
//!
//! ## Error Handling
//!
//! All operations return [`Result<T>`] which is an alias for `Result<T, Error>`.
//! The [`Error`] enum provides comprehensive error reporting for all file system operations.

#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

#[cfg(feature = "std")]
extern crate std;

mod context;
mod devices;
mod error;
mod fundamentals;
pub mod mbr;
mod operations;
mod time;

pub use error::*;

pub use context::*;

pub use devices::*;
pub use fundamentals::*;
pub use operations::file_system::tests;
pub use operations::*;
pub use time::*;

pub const XILA_DISK_SIGNATURE: u32 = u32::from_le_bytes(*b"Xila");
