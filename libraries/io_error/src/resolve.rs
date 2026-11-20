//! Typed resolver failures.
//!
//! DNS lookups report their own fieldless error rather than a bare [`Kind`]:
//! callers can tell a malformed name from an unavailable network, and a
//! confirmed negative answer from an unknown failure. Each variant carries the
//! stable ID of the [`Kind`] it projects to.

use core::fmt;

use crate::{Kind, UnknownKind};

macro_rules! define_resolve_errors {
    ($($name:ident = $kind:ident => $gai:ident;)*) => {
        /// A resolver failure with a stable Xila ID (equal to its [`Kind`]'s).
        #[derive(Clone, Copy, PartialEq, Eq)]
        #[non_exhaustive]
        #[repr(u16)]
        pub enum ResolveError {
            $($name = Kind::$kind as u16,)*
        }

        impl ResolveError {
            /// All resolver errors, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];

            /// Return the stable Xila ID, identical to the projected [`Kind`] ID.
            pub const fn id(self) -> u16 {
                self as u16
            }

            /// Project to the general I/O kind used for device files and errno.
            pub const fn kind(self) -> Kind {
                match self {
                    $(Self::$name => Kind::$kind,)*
                }
            }

            /// Project to the `getaddrinfo` return domain.
            pub const fn getaddrinfo(self) -> GetAddrInfoError {
                match self {
                    $(Self::$name => GetAddrInfoError::$gai,)*
                }
            }
        }

        impl TryFrom<u16> for ResolveError {
            type Error = UnknownKind;

            fn try_from(id: u16) -> Result<Self, Self::Error> {
                $(if id == Kind::$kind as u16 { return Ok(Self::$name); })*
                Err(UnknownKind(id))
            }
        }
    };
}

/// Symbolic `getaddrinfo` failure codes (`EAI_*`). Numeric values belong to
/// the target libc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[allow(non_camel_case_types, reason = "spell EAI symbols as in libc headers")]
pub enum GetAddrInfoError {
    /// `EAI_NONAME`
    EAI_NONAME,
    /// `EAI_FAIL`
    EAI_FAIL,
    /// `EAI_FAMILY`
    EAI_FAMILY,
    /// `EAI_AGAIN`
    EAI_AGAIN,
    /// `EAI_MEMORY`
    EAI_MEMORY,
    /// `EAI_SYSTEM`: the cause is the errno of [`ResolveError::kind`].
    EAI_SYSTEM,
}

define_resolve_errors! {
    InvalidName = InvalidDnsName => EAI_NONAME;
    NameTooLong = NameTooLong => EAI_NONAME;
    UnsupportedQuery = UnsupportedDnsQuery => EAI_FAIL;
    UnsupportedFamily = AddressFamilyUnsupported => EAI_FAMILY;
    NoAnswer = NameNotResolved => EAI_NONAME;
    TemporaryFailure = ResolverTemporaryFailure => EAI_AGAIN;
    Failure = ResolverFailure => EAI_FAIL;
    OutOfMemory = OutOfMemory => EAI_MEMORY;
    NoQuerySlots = NoBuffers => EAI_AGAIN;
    NoInterface = DeviceUnavailable => EAI_SYSTEM;
    NetworkDown = NetworkDown => EAI_SYSTEM;
    NetworkUnreachable = NetworkUnreachable => EAI_SYSTEM;
    TimedOut = TimedOut => EAI_AGAIN;
    SystemIo = InputOutput => EAI_SYSTEM;
}

impl From<ResolveError> for Kind {
    fn from(error: ResolveError) -> Self {
        error.kind()
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.kind(), f)
    }
}

impl fmt::Debug for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Like `Kind`, keep the variant-name table out of firmware.
        write!(f, "ResolveError({:#06x})", self.id())
    }
}

impl core::error::Error for ResolveError {}

const _: () = assert!(size_of::<ResolveError>() == 2);
