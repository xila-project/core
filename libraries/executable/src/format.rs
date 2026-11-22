use alloc::{boxed::Box, string::String, vec::Vec};
use core::pin::Pin;
use file_system::PathOwned;
use task::{EnvironmentVariable, JoinHandle, SpawnerIdentifier, TaskIdentifier};
use users::{GroupIdentifier, UserIdentifier};
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

/// The user and group a launched program runs as, instead of its parent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub user: UserIdentifier,
    pub group: GroupIdentifier,
}

/// Optional parameters of a launch beyond the program's arguments and streams.
///
/// A handler applies the home capability only if its format supports one. It must
/// apply `environment` in every case and fail the launch if it cannot: an explicit
/// environment is a confinement request, so ignoring it would hand the program the
/// caller's whole environment (fail closed).
///
/// An `identity` is applied to mounted native executables only: the launch of any
/// registered file format (WebAssembly, native images) with an identity is refused,
/// so a format handler never has to apply one (fail closed).
#[derive(Default)]
#[non_exhaustive]
pub struct Launch {
    /// The program's whole environment, replacing the copy its task makes of the
    /// parent's. A later variable overrides an earlier one of the same name.
    pub environment: Option<Vec<EnvironmentVariable>>,
    /// Home directory capability for programs that take one (native executables).
    pub home: Option<Home>,
    /// The user and group of the new task, replacing the ones it inherits from its
    /// parent. Deciding who may run a program as another user is the caller's job
    /// (see [`Launch::with_identity`]).
    pub identity: Option<Identity>,
}

impl Launch {
    /// Give the program this whole environment instead of a copy of its parent's.
    pub fn with_environment(mut self, environment: Vec<EnvironmentVariable>) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Give the program this home directory capability.
    pub fn with_home(mut self, home: Home) -> Self {
        self.home = Some(home);
        self
    }

    /// Run the program as `user` and `group` instead of the caller's identity.
    ///
    /// The identity is set on the new task before the program starts, and only on
    /// that task (and the tasks it starts): the caller's own identity never changes.
    /// The execute permission is checked for `user`. The caller must have
    /// authorized the change itself, for example by authenticating an
    /// administrator, because this function grants it unconditionally.
    pub fn with_identity(mut self, user: UserIdentifier, group: GroupIdentifier) -> Self {
        self.identity = Some(Identity { user, group });
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
