//! The running Xila version and the rule that decides which applications it runs.
//!
//! Applications declare the Xila version they were built for as `major.minor`
//! instead of a separate ABI number (xila-project/core#276). [`RUNNING`] is the
//! one place that states the running version: it is derived at build time from
//! Core's workspace version, so bumping `[workspace.package] version` updates
//! the native ABI note, the native API table and every loader check together.
#![no_std]

use core::fmt;

/// A Xila version as `major.minor`. The patch version never affects compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// The 32-bit form stored in the native ELF ABI note and in `XilaApi.version`:
    /// the major version in the upper 16 bits and the minor version in the lower 16 bits.
    pub const fn packed(self) -> u32 {
        ((self.major as u32) << 16) | self.minor as u32
    }

    /// The inverse of [`Version::packed`]. Every 32-bit value names exactly one version.
    pub const fn from_packed(packed: u32) -> Self {
        Self::new((packed >> 16) as u16, packed as u16)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// Check whether the `running` Xila version can run an application built for `declared`.
///
/// Before 1.0, both versions must have major version zero and the same minor
/// version. From 1.0, the major versions must be equal and the running minor
/// version must be at least the declared one.
///
/// This mirrors `xila_package_manifest::compatible` in package_manager, which
/// Core cannot depend on. Keep both, and their boundary tests, identical.
pub const fn compatible(declared: Version, running: Version) -> bool {
    declared.major == running.major
        && if declared.major == 0 {
            declared.minor == running.minor
        } else {
            declared.minor <= running.minor
        }
}

/// The Xila version this build of Core is, from `[workspace.package] version`.
pub const RUNNING: Version = Version::new(
    parse(env!("CARGO_PKG_VERSION_MAJOR")),
    parse(env!("CARGO_PKG_VERSION_MINOR")),
);

/// Parse a decimal version component at compile time.
const fn parse(text: &str) -> u16 {
    let bytes = text.as_bytes();
    assert!(!bytes.is_empty(), "empty version component");
    let mut value: u32 = 0;
    let mut index = 0;
    while index < bytes.len() {
        let digit = bytes[index];
        assert!(digit.is_ascii_digit(), "non-decimal version component");
        value = value * 10 + (digit - b'0') as u32;
        assert!(value <= u16::MAX as u32, "version component above 65535");
        index += 1;
    }
    value as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        let (major, minor) = text.split_once('.').unwrap();
        Version::new(major.parse().unwrap(), minor.parse().unwrap())
    }

    // The same cases as `xila_package_manifest`'s compatibility test.
    #[test]
    fn checks_compatibility_at_major_and_minor_boundaries() {
        for (declared, running, expected) in [
            ("0.1", "0.1", true),
            ("0.1", "0.2", false),
            ("0.2", "0.1", false),
            ("1.3", "1.2", false),
            ("1.3", "1.3", true),
            ("1.3", "1.9", true),
            ("1.3", "2.0", false),
            ("0.9", "1.0", false),
            ("0.0", "0.0", true),
            ("0.0", "0.1", false),
            ("0.1", "0.0", false),
        ] {
            assert_eq!(
                compatible(version(declared), version(running)),
                expected,
                "{declared} on {running}"
            );
        }
    }

    #[test]
    fn running_version_follows_the_workspace_version() {
        let mut parts = env!("CARGO_PKG_VERSION").split('.');
        assert_eq!(RUNNING.major, parts.next().unwrap().parse().unwrap());
        assert_eq!(RUNNING.minor, parts.next().unwrap().parse().unwrap());
        assert!(compatible(RUNNING, RUNNING));
    }

    #[test]
    fn packs_major_above_minor() {
        assert_eq!(Version::new(0, 1).packed(), 0x0000_0001);
        assert_eq!(Version::new(1, 3).packed(), 0x0001_0003);
        assert_eq!(Version::new(u16::MAX, 2).packed(), 0xffff_0002);
        for packed in [0, 1, 0x0001_0003, 0x8000_ffff, u32::MAX] {
            assert_eq!(Version::from_packed(packed).packed(), packed);
        }
    }

    #[test]
    fn parses_decimal_components() {
        assert_eq!(parse("0"), 0);
        assert_eq!(parse("12"), 12);
        assert_eq!(parse("65535"), u16::MAX);
    }

    #[test]
    fn displays_major_and_minor() {
        extern crate std;
        use std::string::ToString;
        assert_eq!(Version::new(1, 3).to_string(), "1.3");
    }
}
