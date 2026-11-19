use alloc::{boxed::Box, string::String, vec::Vec};
use core::pin::Pin;
use task::{JoinHandle, SpawnerIdentifier, TaskIdentifier};
use virtual_file_system::File;

use crate::{Error, Result, Standard};

pub(crate) const ELF: [u8; 4] = *b"\x7fELF";

/// Platform registration for a four-byte file magic. The context searches
/// registrations in order. Mounted executables continue to use their control command.
pub struct ExecutableFormat {
    magic: [u8; 4],
    handler: &'static dyn FormatHandler,
}

impl ExecutableFormat {
    pub const fn new(magic: [u8; 4], handler: &'static dyn FormatHandler) -> Self {
        Self { magic, handler }
    }
}

#[non_exhaustive]
pub struct FormatRequest {
    /// The already-opened executable file, positioned at the beginning.
    pub file: File<'static>,
    pub file_size: file_system::Size,
    pub name: String,
    pub parent: TaskIdentifier,
    pub spawner: Option<SpawnerIdentifier>,
    pub standard: Standard<'static>,
    pub arguments: Vec<String>,
}

/// Start a registered file format and return the child's exit-status handle.
pub trait FormatHandler: Sync {
    fn launch(&self, request: FormatRequest) -> FormatLaunch<'_>;
}

pub type FormatLaunch<'a> = Pin<Box<dyn Future<Output = Result<JoinHandle<isize>>> + 'a>>;

pub(crate) fn find_format(
    formats: &[ExecutableFormat],
    magic: [u8; 4],
    setuid: bool,
) -> Result<Option<&'static dyn FormatHandler>> {
    let handler = formats
        .iter()
        .find(|format| format.magic == magic)
        .map(|format| format.handler);
    if (magic == ELF && (!cfg!(feature = "native-dev") || handler.is_none()))
        || (handler.is_some() && setuid)
    {
        return Err(Error::PermissionDenied);
    }
    Ok(handler)
}
