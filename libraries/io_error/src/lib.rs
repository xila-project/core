//! Common errors for file systems, device I/O and network transport.
//!
//! [`Kind`] is the single failure vocabulary: two bytes, an explicit stable ID,
//! a symbolic POSIX projection ([`Kind::posix_errno`]), an `embedded_io`
//! classification and a localized message. [`ResolveError`] is the typed DNS
//! result. See the crate README for the ID bands and the rules for choosing a kind.
#![no_std]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;
use embedded_io::ErrorKind;

embedded_i18n::i18n!("locales");

mod display;
mod posix;
mod resolve;
#[cfg(feature = "std")]
mod std_error;

pub use posix::PosixErrno;
pub use resolve::{GetAddrInfoError, ResolveError};
#[cfg(feature = "std")]
pub use std_error::map_std_error;

macro_rules! define_kinds {
    ($($kind:ident = $id:literal => $errno:ident, $embedded:ident;)*) => {
        /// A semantic I/O failure with a stable Xila ID.
        #[derive(Clone, Copy, PartialEq, Eq)]
        #[non_exhaustive]
        #[repr(u16)]
        pub enum Kind {
            $($kind = $id,)*
        }

        impl Kind {
            /// All assigned kinds, in ID order.
            pub const ALL: &'static [Self] = &[$(Self::$kind,)*];

            /// Return the stable Xila ID. Zero is reserved for success.
            pub const fn id(self) -> u16 {
                self as u16
            }

            /// Project to a symbolic POSIX errno. The caller supplies target
            /// numbers and applies any operation-specific policy.
            pub const fn posix_errno(self) -> PosixErrno {
                match self {
                    $(Self::$kind => PosixErrno::$errno,)*
                }
            }

            /// Variant spelling for the native ABI generator.
            #[doc(hidden)]
            pub const fn variant_name(self) -> &'static str {
                match self {
                    $(Self::$kind => stringify!($kind),)*
                }
            }
        }

        impl TryFrom<u16> for Kind {
            type Error = UnknownKind;

            fn try_from(id: u16) -> Result<Self, Self::Error> {
                match id {
                    $($id => Ok(Self::$kind),)*
                    _ => Err(UnknownKind(id)),
                }
            }
        }

        impl embedded_io::Error for Kind {
            fn kind(&self) -> ErrorKind {
                match self {
                    $(Self::$kind => ErrorKind::$embedded,)*
                }
            }
        }
    };
}

include!("catalog.rs");

/// A reserved or unknown Xila error ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownKind(pub u16);

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown I/O error ID {:#06x}", self.0)
    }
}

impl core::error::Error for UnknownKind {}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keep the variant-name table out of firmware using Debug.
        write!(f, "IoErrorKind({:#06x})", self.id())
    }
}

impl core::error::Error for Kind {}

const _: () = {
    assert!(size_of::<Kind>() == 2);
    assert!(size_of::<Result<(), Kind>>() == 2);
    #[cfg(target_pointer_width = "32")]
    {
        assert!(size_of::<Result<usize, Kind>>() == 8);
        assert!(size_of::<Result<u64, Kind>>() <= 16);
    }
};
