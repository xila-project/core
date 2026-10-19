//! File position types for seek operations.
//!
//! This module provides the [`Position`] enumeration for specifying
//! file positions during seek operations. It supports absolute positioning
//! from the start or end of files, as well as relative positioning from
//! the current file cursor location.

/// Represents a position within a file for seek operations.
///
/// This enum provides three different ways to specify a position within a file:
/// - Absolute position from the beginning of the file
/// - Relative position from the current cursor location
/// - Relative position from the end of the file
///
/// # Examples
///
/// ```rust
/// use file_system::Position;
///
/// // Seek to the beginning of the file
/// let start = Position::Start(0);
///
/// // Seek 100 bytes from the beginning
/// let absolute = Position::Start(100);
///
/// // Move 50 bytes forward from current position
/// let forward = Position::Current(50);
///
/// // Move 20 bytes backward from current position
/// let backward = Position::Current(-20);
///
/// // Seek to 10 bytes before the end of the file
/// let near_end = Position::End(-10);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// Absolute position from the start of the file.
    ///
    /// The value represents the number of bytes from the beginning of the file.
    /// A value of 0 positions at the very start of the file.
    Start(u64),

    /// Relative position from the current cursor location.
    ///
    /// Positive values move forward, negative values move backward.
    /// A value of 0 keeps the current position unchanged.
    Current(i64),

    /// Relative position from the end of the file.
    ///
    /// Negative values position before the end, positive values would position
    /// beyond the end (which may extend the file if writing is performed).
    /// A value of 0 positions exactly at the end of the file.
    End(i64),
}
