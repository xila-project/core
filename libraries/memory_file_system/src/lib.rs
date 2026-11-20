//! In-RAM file system for device nodes, named pipes, and directory hierarchies.
//!
//! `memory_file_system` implements [`::file_system::FileSystemOperations`] entirely in RAM
//! without touching persistent flash storage. It is designed to be mounted at `/devices`
//! so that device nodes, pipes, and sub-directories created at boot do not cause flash
//! writes or persist stale inode mappings across reboots.
//!
//! Regular file contents are not supported: [`::file_system::FileSystemOperations::lookup_file`]
//! and [`::file_system::BaseOperations`] I/O methods return
//! [`::file_system::IoErrorKind::Unsupported`].
//!
//! # Memory Layout and Per-Node Cost
//!
//! All nodes are stored in a single flat `Vec<Option<Node>>` indexed by a 32-bit node index,
//! with each node holding its parent index, a generation counter (to detect stale directory
//! handles after node removal and slot reuse), a compact boxed name (`Box<str>`), and stored
//! metadata fields (no per-node `BTreeMap` or file content buffer). Because `Box<str>` carries
//! a non-null pointer niche, `Option<Node>` adds zero discriminant overhead.
//!
//! Exact per-node RAM cost (excluding `Vec` capacity slack and heap allocator metadata):
//! - **32-bit targets** (for example Cortex-M4 with 256 KB RAM): **64 bytes** per `Option<Node>`
//!   plus `name.len()` heap bytes for the node name (`0` heap bytes for the root directory).
//! - **64-bit targets** (for example x86_64 host): **72 bytes** per `Option<Node>`
//!   plus `name.len()` heap bytes for the node name (`0` heap bytes for the root directory).

#![no_std]

extern crate alloc;

mod attributes;
mod directory;
mod file_system;
mod node;

pub use file_system::FileSystem;
