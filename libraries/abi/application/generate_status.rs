//! Generate both the Rust status and cbindgen's input from the I/O kind catalog.

use std::{fmt::Write, fs, path::Path};

use io_error::Kind;

pub fn generate(output: &Path) {
    let mut source = String::from(
        "/// Native status values mirror the unified error kinds one-to-one. Zero is success.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         #[repr(u32)]\n\
         pub enum Status {\n    Ok = 0,\n",
    );
    for &kind in Kind::ALL {
        writeln!(source, "    {} = {:#06x},", status_name(kind), kind.id()).unwrap();
    }
    source.push_str("    End = 0x0701,\n    BufferTooSmall = 0x0702,\n}\n");

    source.push_str(
        "impl Status {\n\
         /// Recover the I/O kind, if this status represents an error in that domain.\n\
         pub const fn io_kind(self) -> Option<file_system::IoErrorKind> {\n\
         use file_system::IoErrorKind as Kind;\n\
         match self {\n\
         Self::Ok | Self::End | Self::BufferTooSmall => None,\n",
    );
    for &kind in Kind::ALL {
        writeln!(
            source,
            "Self::{} => Some(Kind::{}),",
            status_name(kind),
            kind.variant_name(),
        )
        .unwrap();
    }
    source.push_str(
        "}\n}\n}\n\
         impl From<file_system::IoErrorKind> for Status {\n\
         fn from(kind: file_system::IoErrorKind) -> Self {\n\
         use file_system::IoErrorKind as Kind;\n\
         match kind {\n",
    );
    for &kind in Kind::ALL {
        writeln!(
            source,
            "Kind::{} => Self::{},",
            kind.variant_name(),
            status_name(kind),
        )
        .unwrap();
    }
    // Kind is non-exhaustive across crates. The arms above cover the whole
    // catalog of the same crate version, and a test freezes that table.
    // Never panic at the ABI boundary: an unknown future kind is a generic failure.
    source.push_str(
        "_ => Self::InputOutput,\n}\n}\n}\n\
         impl From<file_system::io_error::ResolveError> for Status {\n\
         fn from(error: file_system::io_error::ResolveError) -> Self {\n\
         Self::from(error.kind())\n}\n}\n",
    );
    fs::write(output, source).unwrap();
}

fn status_name(kind: Kind) -> &'static str {
    match kind {
        Kind::DeviceUnavailable => "Unavailable",
        other => other.variant_name(),
    }
}
