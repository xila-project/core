use alloc::{boxed::Box, string::String, vec::Vec};
use core::pin::Pin;
use file_system::PathOwned;
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

/// A directory capability a launched program gets as its home, bound to its own task.
pub struct Home {
    pub path: PathOwned,
    pub rights: u8,
}

/// Optional parameters of a launch beyond the program's arguments and streams.
/// A handler applies the ones its format supports; unsupported ones are ignored.
#[derive(Default)]
#[non_exhaustive]
pub struct Launch {
    /// The program's whole environment, replacing the copy its task makes of the
    /// parent's. Name and value pairs; a later pair overrides an earlier one.
    pub environment: Option<Vec<(String, String)>>,
    /// Home directory capability for programs that take one (native executables).
    pub home: Option<Home>,
}

impl Launch {
    /// Give the program this whole environment instead of a copy of its parent's.
    pub fn with_environment(mut self, environment: Vec<(String, String)>) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Give the program this home directory capability.
    pub fn with_home(mut self, home: Home) -> Self {
        self.home = Some(home);
        self
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
    pub launch: Launch,
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
