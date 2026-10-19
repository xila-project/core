//! Time representation and utilities for file system operations.
//!
//! This module provides time-related types and functionality specifically designed
//! for file system metadata operations, including creation times, modification times,
//! and access times.

use core::fmt::{self, Display, Formatter};
use core::time::Duration;
use shared::decompose_unix_timestamp;

/// Represents a point in time for file system operations.
///
/// `Time` stores time as seconds since the Unix epoch (January 1, 1970).
/// It's used throughout the file system for tracking file creation, modification,
/// and access times. The type is designed to be efficient for storage and comparison.
///
/// # Examples
///
/// ```rust
/// # extern crate alloc;
/// use file_system::Time;
///
/// // Create a time representing the Unix epoch
/// let epoch = Time::new(0);
/// assert_eq!(epoch.as_u64(), 0);
///
/// // Create a time for a specific moment
/// let time = Time::new(1642684800); // January 20, 2022
/// ```
///
/// # Storage
///
/// Times are stored as 64-bit unsigned integers representing seconds, providing
/// a range from 1970 to approximately year 584 billion, which is sufficient
/// for any practical file system use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Time {
    seconds: u64,
}

impl Time {
    /// Create a new time from seconds since Unix epoch.
    ///
    /// # Arguments
    ///
    /// * `Seconds` - Number of seconds since January 1, 1970 00:00:00 UTC
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::Time;
    ///
    /// let time = Time::new(1640995200); // January 1, 2022
    /// ```
    pub const fn new(seconds: u64) -> Self {
        Self { seconds }
    }

    /// Get the time as seconds since Unix epoch.
    ///
    /// # Returns
    ///
    /// Number of seconds since January 1, 1970 00:00:00 UTC.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::Time;
    ///
    /// let time = Time::new(1640995200);
    /// assert_eq!(time.as_u64(), 1640995200);
    /// ```
    pub const fn as_u64(self) -> u64 {
        self.seconds
    }
}

/// Convert from a duration to a time.
///
/// This treats the duration as an absolute time since the Unix epoch.
impl From<Duration> for Time {
    fn from(duration: Duration) -> Self {
        Self {
            seconds: duration.as_secs(),
        }
    }
}

/// Convert from a time to a duration.
///
/// This converts the absolute time to a duration since the Unix epoch.
impl From<Time> for Duration {
    fn from(time: Time) -> Self {
        Duration::new(time.seconds, 0)
    }
}

impl Display for Time {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let (year, month, day, hour, minute, second) =
            decompose_unix_timestamp(self.seconds as i64);

        write!(
            f,
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    #[test]
    fn test_time_creation() {
        let time = Time::new(1640995200); // January 1, 2022
        assert_eq!(time.as_u64(), 1640995200);
    }

    #[test]
    fn test_time_epoch() {
        let epoch = Time::new(0);
        assert_eq!(epoch.as_u64(), 0);
    }

    #[test]
    fn test_time_const_operations() {
        // Test that New and as_u64 are const functions
        const TIME: Time = Time::new(1234567890);
        const SECONDS: u64 = TIME.as_u64();

        assert_eq!(SECONDS, 1234567890);
        assert_eq!(TIME.as_u64(), 1234567890);
    }

    #[test]
    fn test_time_comparison() {
        let early = Time::new(1000);
        let late = Time::new(2000);

        assert!(early < late);
        assert!(late > early);
        assert!(early <= late);
        assert!(late >= early);
        assert!(early <= early);
        assert!(late >= late);
        assert_eq!(early, early);
        assert_ne!(early, late);
    }

    #[test]
    fn test_time_ordering() {
        let mut times = [
            Time::new(3000),
            Time::new(1000),
            Time::new(2000),
            Time::new(500),
        ];

        times.sort();

        assert_eq!(times[0], Time::new(500));
        assert_eq!(times[1], Time::new(1000));
        assert_eq!(times[2], Time::new(2000));
        assert_eq!(times[3], Time::new(3000));
    }

    #[test]
    fn test_time_clone_copy() {
        let original = Time::new(999);
        let cloned = original;
        let copied = original;

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
        assert_eq!(cloned, copied);

        // Test that we can still use original after copying
        assert_eq!(original.as_u64(), 999);
    }

    #[test]
    fn test_time_debug() {
        let time = Time::new(1640995200);
        let debug_str = format!("{time:?}");
        assert!(debug_str.contains("Time"));
        assert!(debug_str.contains("1640995200"));
    }

    #[test]
    fn test_time_hash() {
        use alloc::collections::BTreeMap;

        let time1 = Time::new(12345);
        let time2 = Time::new(12345);
        let time3 = Time::new(54321);

        // Test that equal times can be used as keys in collections
        let mut map = BTreeMap::new();
        map.insert(time1, "first");
        map.insert(time2, "second"); // Should overwrite first
        map.insert(time3, "third");

        assert_eq!(map.len(), 2); // time1 and time2 are equal, so only 2 entries
        assert_eq!(map.get(&time1), Some(&"second"));
        assert_eq!(map.get(&time3), Some(&"third"));
    }

    #[test]
    fn test_time_from_duration() {
        let duration = Duration::new(1640995200, 0);
        let time: Time = duration.into();
        assert_eq!(time.as_u64(), 1640995200);
    }

    #[test]
    fn test_time_to_duration() {
        let time = Time::new(1640995200);
        let duration: Duration = time.into();
        assert_eq!(duration.as_secs(), 1640995200);
    }

    #[test]
    fn test_time_display_formatting() {
        // Test display formatting
        let time = Time::new(0); // Unix epoch
        let display_str = format!("{time}");

        // The exact format depends on Unix_to_human_time implementation
        // We just verify it produces some reasonable format
        assert!(display_str.contains("-"));
        assert!(display_str.contains(":"));
        assert!(display_str.len() > 10); // Should be a reasonable datetime string
    }

    #[test]
    fn test_time_display_various_dates() {
        // Test some known timestamps
        let times = vec![
            Time::new(0),          // 1970-01-01 00:00:00
            Time::new(86400),      // 1970-01-02 00:00:00
            Time::new(1640995200), // 2022-01-01 00:00:00 (approximately)
        ];

        for time in times {
            let display_str = format!("{time}");
            // Basic sanity checks
            assert!(display_str.len() >= 19); // YYYY-MM-DD HH:MM:SS is 19 chars
            assert!(display_str.contains("-"));
            assert!(display_str.contains(":"));
            assert!(display_str.contains(" "));
        }
    }

    #[test]
    fn test_time_max_value() {
        let max_time = Time::new(u64::MAX);
        assert_eq!(max_time.as_u64(), u64::MAX);

        // Should still be convertible to duration
        let duration: Duration = max_time.into();
        assert_eq!(duration.as_secs(), u64::MAX);
    }

    #[test]
    fn test_time_zero_and_max_comparison() {
        let zero = Time::new(0);
        let max = Time::new(u64::MAX);

        assert!(zero < max);
        assert!(max > zero);
        assert_ne!(zero, max);
    }

    #[test]
    fn test_time_round_trip_conversions() {
        let original_seconds = 1640995200u64;

        // Time -> Duration -> Time
        let time = Time::new(original_seconds);
        let duration: Duration = time.into();
        let back_to_time: Time = duration.into();

        assert_eq!(time, back_to_time);
        assert_eq!(original_seconds, back_to_time.as_u64());
    }

    #[test]
    fn test_time_type_size() {
        use core::mem::{align_of, size_of};

        // Should be same size as u64 due to repr(transparent)
        assert_eq!(size_of::<Time>(), size_of::<u64>());
        assert_eq!(align_of::<Time>(), align_of::<u64>());
    }

    #[test]
    fn test_time_sequence() {
        // Test a sequence of times
        use alloc::vec::Vec;
        let times: Vec<Time> = (0..10)
            .map(|i| Time::new(i * 86400)) // Each day
            .collect();

        // Verify they're in ascending order
        for i in 1..times.len() {
            assert!(times[i - 1] < times[i]);
        }
    }
}
