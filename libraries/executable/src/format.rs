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

/// The network rights a launch grants a native process: connecting (and sending
/// datagrams), listening (and binding) and resolving names. A launch grants none by
/// default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkRights(u8);

impl NetworkRights {
    /// No network: the default.
    pub const NONE: Self = Self(0);
    /// Open TCP connections and send UDP datagrams.
    pub const CONNECT: Self = Self(1);
    /// Listen for TCP connections and bind UDP sockets.
    pub const LISTEN: Self = Self(2);
    /// Resolve host names.
    pub const RESOLVE: Self = Self(4);
    /// Every right.
    pub const ALL: Self = Self(7);

    /// The rights in either set.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every right of `other` is in this set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The rights as the native ABI's `XILA_NETWORK_*` bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The set from `XILA_NETWORK_*` bits, or `None` if an unknown bit is set.
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }
}

impl core::ops::BitOr for NetworkRights {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
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
///
/// `network` is denied by default: a program gets network authority only from an
/// explicit [`Launch::with_network`], and only formats with a socket interface apply
/// it (native executables, through `XilaStartup.network`). WebAssembly applications
/// get no sockets, so a grant has no effect on them.
#[derive(Default)]
#[non_exhaustive]
pub struct Launch {
    /// The program's whole environment, replacing the copy its task makes of the
    /// parent's. A later variable overrides an earlier one of the same name.
    pub environment: Option<Vec<EnvironmentVariable>>,
    /// Home directory capability for programs that take one (native executables).
    pub home: Option<Home>,
    #[cfg(feature = "native-dev")]
    pub(crate) pinned_home: Option<abi_application::HomeGrant>,
    /// The user and group of the new task, replacing the ones it inherits from its
    /// parent. Deciding who may run a program as another user is the caller's job
    /// (see [`Launch::with_identity`]).
    pub identity: Option<Identity>,
    /// The network rights of the program. None by default (see [`Launch::with_network`]).
    pub network: NetworkRights,
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
    /// The new task and all its descendants are marked as running with a granted
    /// identity, which they cannot clear (`task::Manager::is_identity_granted`), so
    /// runtimes for untrusted code can refuse to run under it.
    /// The execute permission is checked for `user`. The caller must have
    /// authorized the change itself, for example by authenticating an
    /// administrator, because this function grants it unconditionally.
    pub fn with_identity(mut self, user: UserIdentifier, group: GroupIdentifier) -> Self {
        self.identity = Some(Identity { user, group });
        self
    }

    /// Grant the program these network rights. Without this call it has no network.
    ///
    /// A native process that spawns a child can pass it a subset of these rights and
    /// never more. Deciding which programs may use the network is the caller's job:
    /// this function grants the rights unconditionally.
    pub fn with_network(mut self, rights: NetworkRights) -> Self {
        self.network = rights;
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

/// Start a registered file format and return the child's exit-status handle and task.
pub trait FormatHandler: Sync {
    fn launch(&self, request: FormatRequest) -> FormatLaunch<'_>;
}

pub type FormatLaunch<'a> =
    Pin<Box<dyn Future<Output = Result<(JoinHandle<isize>, TaskIdentifier)>> + 'a>>;

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
