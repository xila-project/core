//! Real-time clock device contract (`/devices/time/real_time_clock`, kind `b'r'`).
//!
//! A real-time clock (RTC) stores wall-clock time (UTC) across resets and, on
//! chips with a battery-backed domain, across power-off. Like Linux, Xila keeps
//! RTC drivers separate from clocksource drivers: the RTC only supplies and
//! stores UTC; timers, deadlines and uptime stay on the monotonic clock of the
//! `time` subsystem and never read this device.
//!
//! The upper half (`drivers_peripherals::real_time_clock`) adapts a
//! `peripherals::real_time_clock::RealTimeClockController` lower half to this
//! contract. Plain `read` and `write` on the node fail with `Unsupported`:
//! the clock is only reachable through the control commands below.
//!
//! # Control commands (kind `b'r'`)
//!
//! | Number | Command    | Direction | Input                  | Output                 | Effect                                              |
//! |--------|------------|-----------|------------------------|------------------------|-----------------------------------------------------|
//! | `0x00` | `GET_TIME` | Read      | `RealTimeClockTimeAbi` | `RealTimeClockTimeAbi` | Returns the time the clock holds and its validity.  |
//! | `0x01` | `SET_TIME` | Write     | `RealTimeClockTimeAbi` | `RealTimeClockTimeAbi` | Sets the clock, then returns the time it now holds. |
//!
//! Both commands carry the same versioned payload. In a `GET_TIME` input only
//! `version` and `reserved` are checked; the time fields are ignored. A wrong
//! `version`, a non-zero `reserved` or, for `SET_TIME`, `valid != 1` or
//! `nanoseconds >= 1_000_000_000` fails with `InvalidArgument`. In a `GET_TIME`
//! output `valid` is `1` when the clock holds a valid time and `0` when it does
//! not (for example after power-off on a chip without a battery-backed domain);
//! then `seconds` and `nanoseconds` are zero. A clock that cannot be read or
//! written fails with `InputOutput`.

use file_system::{ControlCommand, define_command};

/// Path of the real-time clock device node in the virtual file system.
pub const REAL_TIME_CLOCK_PATH: &str = "/devices/time/real_time_clock";

/// Version of the real-time clock control ABI payload.
pub const ABI_VERSION: u16 = 1;

/// `valid` value of a clock that holds a valid time.
pub const VALID: u8 = 1;
/// `valid` value of a clock that holds no valid time.
pub const NOT_VALID: u8 = 0;

/// Largest accepted value of `nanoseconds` (exclusive).
pub const MAX_NANOSECONDS: u32 = 1_000_000_000;

file_system::plain_data! {
    #[repr(C)]
    /// UTC wall-clock time held by a real-time clock, plus its ABI version and
    /// validity. Field order keeps the struct free of padding on 32-bit and
    /// 64-bit targets: 16 bytes everywhere.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct RealTimeClockTimeAbi {
        pub version: u16,
        /// [`VALID`] when the clock holds a valid time, [`NOT_VALID`] otherwise.
        pub valid: u8,
        pub reserved: u8,
        /// Nanoseconds within the second, `0..`[`MAX_NANOSECONDS`].
        pub nanoseconds: u32,
        /// Signed Unix seconds since 1970-01-01T00:00:00Z, without timezone or DST.
        pub seconds: i64,
    }
}

define_command!(
    GET_TIME,
    Read,
    crate::kind::REAL_TIME_CLOCK,
    0x00,
    RealTimeClockTimeAbi,
    RealTimeClockTimeAbi
);
define_command!(
    SET_TIME,
    Write,
    crate::kind::REAL_TIME_CLOCK,
    0x01,
    RealTimeClockTimeAbi,
    RealTimeClockTimeAbi
);

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};
    use file_system::ControlDirectionFlags;

    #[test]
    fn identifiers_are_distinct_and_directions_match_the_contract() {
        assert_ne!(GET_TIME::IDENTIFIER, SET_TIME::IDENTIFIER);
        assert_eq!(
            GET_TIME::IDENTIFIER.get_kind(),
            crate::kind::REAL_TIME_CLOCK
        );
        assert_eq!(
            SET_TIME::IDENTIFIER.get_kind(),
            crate::kind::REAL_TIME_CLOCK
        );
        assert_eq!(GET_TIME::IDENTIFIER.get_number(), 0x00);
        assert_eq!(SET_TIME::IDENTIFIER.get_number(), 0x01);
        assert!(
            GET_TIME::IDENTIFIER
                .get_direction()
                .contains(ControlDirectionFlags::Read)
        );
        assert!(
            SET_TIME::IDENTIFIER
                .get_direction()
                .contains(ControlDirectionFlags::Write)
        );
        assert!(
            !GET_TIME::IDENTIFIER
                .get_direction()
                .contains(ControlDirectionFlags::Write)
        );
    }

    #[test]
    fn payload_is_sixteen_bytes_without_padding_on_any_target() {
        // plain_data! already rejects padding at compile time; pin the layout so
        // a field reorder cannot silently change the wire size.
        assert_eq!(size_of::<RealTimeClockTimeAbi>(), 16);
        assert_eq!(offset_of!(RealTimeClockTimeAbi, version), 0);
        assert_eq!(offset_of!(RealTimeClockTimeAbi, valid), 2);
        assert_eq!(offset_of!(RealTimeClockTimeAbi, reserved), 3);
        assert_eq!(offset_of!(RealTimeClockTimeAbi, nanoseconds), 4);
        assert_eq!(offset_of!(RealTimeClockTimeAbi, seconds), 8);
    }

    #[test]
    fn payload_round_trips_through_raw_wire_bytes() {
        let time = RealTimeClockTimeAbi {
            version: ABI_VERSION,
            valid: VALID,
            reserved: 0,
            nanoseconds: 123_456_789,
            seconds: -1,
        };
        // The payload crosses the host/guest boundary as raw bytes. It is plain
        // data without padding, so reading it as bytes is sound.
        let mut bytes = [0u8; 16];
        // SAFETY: `RealTimeClockTimeAbi` is `PlainData` and has no padding.
        unsafe {
            core::ptr::copy_nonoverlapping(
                &time as *const RealTimeClockTimeAbi as *const u8,
                bytes.as_mut_ptr(),
                bytes.len(),
            )
        };
        // Little-endian wire layout of the pinned field order.
        assert_eq!(&bytes[0..2], &ABI_VERSION.to_le_bytes());
        assert_eq!(bytes[2], VALID);
        assert_eq!(bytes[3], 0);
        assert_eq!(&bytes[4..8], &123_456_789u32.to_le_bytes());
        assert_eq!(&bytes[8..16], &(-1i64).to_le_bytes());
        // SAFETY: every byte pattern is a valid `RealTimeClockTimeAbi`.
        let decoded: RealTimeClockTimeAbi =
            unsafe { core::ptr::read(bytes.as_ptr() as *const RealTimeClockTimeAbi) };
        assert_eq!(decoded, time);
        // Any byte pattern decodes: the payload is plain data, so a guest can
        // never send an invalid value such as `2` for the validity byte.
        let garbage = [0xFFu8; 16];
        // SAFETY: every byte pattern is valid.
        let _: RealTimeClockTimeAbi =
            unsafe { core::ptr::read(garbage.as_ptr() as *const RealTimeClockTimeAbi) };
    }
}
