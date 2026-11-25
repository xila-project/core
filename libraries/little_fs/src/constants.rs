//! LittleFS enumeration constants with one Rust type on every host.
//!
//! bindgen gives each C enumeration constant the enumeration's underlying
//! type, which the C compiler picks per target: `unsigned int` (`u32`) with
//! GCC or Clang on Linux, but `int` (`i32`) on Windows (MSVC). Comparing or
//! combining them directly with the `uint32_t` and `uint8_t` fields of the
//! LittleFS structures therefore compiles on one host and not the other.
//!
//! Every constant below is cast once to the type of the C field or argument
//! it is used with, so the rest of the crate never depends on the type bindgen
//! picked. Use these instead of the raw `lfs_open_flags_*`, `lfs_type_*` and
//! `lfs_whence_flags_*` bindings. (`lfs_error_*` has negative values, so it is
//! `int` on every host and needs no alias.)

#![allow(
    clippy::unnecessary_cast,
    reason = "a cast that changes nothing on one host converts the type on another"
)]

use crate::littlefs;

// `flags` argument of `lfs_file_open` and `lfs_file_opencfg` (`int`).

pub const OPEN_READ_ONLY: i32 = littlefs::lfs_open_flags_LFS_O_RDONLY as i32;
#[cfg(test)]
pub const OPEN_WRITE_ONLY: i32 = littlefs::lfs_open_flags_LFS_O_WRONLY as i32;
pub const OPEN_READ_WRITE: i32 = littlefs::lfs_open_flags_LFS_O_RDWR as i32;
pub const OPEN_CREATE: i32 = littlefs::lfs_open_flags_LFS_O_CREAT as i32;
pub const OPEN_EXCLUSIVE: i32 = littlefs::lfs_open_flags_LFS_O_EXCL as i32;
pub const OPEN_TRUNCATE: i32 = littlefs::lfs_open_flags_LFS_O_TRUNC as i32;
pub const OPEN_APPEND: i32 = littlefs::lfs_open_flags_LFS_O_APPEND as i32;

// `flags` field of `lfs_file_t` (`uint32_t`).

/// Bits of the access mode (`LFS_O_RDONLY`, `LFS_O_WRONLY` or `LFS_O_RDWR`).
pub const FILE_ACCESS_MASK: u32 = littlefs::lfs_open_flags_LFS_O_RDWR as u32;
pub const FILE_READ_ONLY: u32 = littlefs::lfs_open_flags_LFS_O_RDONLY as u32;
/// The file does not match storage because of a write.
pub const FILE_DIRTY: u32 = littlefs::lfs_open_flags_LFS_F_DIRTY as u32;
/// The file has data written since the last flush. LittleFS sets this, not
/// `LFS_F_DIRTY`, on `lfs_file_write`, so pending state must check both.
pub const FILE_WRITING: u32 = littlefs::lfs_open_flags_LFS_F_WRITING as u32;
/// The file has uncommitted data or metadata (`LFS_F_WRITING | LFS_F_DIRTY`):
/// the next sync or close commits it.
pub const FILE_PENDING: u32 = FILE_DIRTY | FILE_WRITING;
/// The file does not match storage because another open changed it.
pub const FILE_DUSTY: u32 = littlefs::lfs_open_flags_LFS_F_DUSTY as u32;
/// A write or commit failed; LittleFS no longer syncs the file.
pub const FILE_ERRORED: u32 = littlefs::lfs_open_flags_LFS_F_ERRED as u32;

// `type` field of `lfs_info` and `lfs_mlist` (`uint8_t`).

pub const TYPE_REGULAR: u8 = littlefs::lfs_type_LFS_TYPE_REG as u8;
pub const TYPE_DIRECTORY: u8 = littlefs::lfs_type_LFS_TYPE_DIR as u8;

// `whence` argument of `lfs_file_seek` (`int`).

pub const SEEK_START: i32 = littlefs::lfs_whence_flags_LFS_SEEK_SET as i32;
pub const SEEK_CURRENT: i32 = littlefs::lfs_whence_flags_LFS_SEEK_CUR as i32;
pub const SEEK_END: i32 = littlefs::lfs_whence_flags_LFS_SEEK_END as i32;
