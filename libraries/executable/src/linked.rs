//! Statically linked native programs.
//!
//! A linked program is a native entry (`int32_t entry(const XilaStartup *)`) compiled and
//! linked into the firmware. It runs as a full native process: the same startup block, the
//! same process capability and the same cleanup as a program that the native loader loads
//! from a file, but with no ELF file, no loader and no executable memory.
//!
//! Each running program owns one stackful coroutine, one process heap and one host state.
//! Programs run concurrently on the executor of the task that started them. A program finds
//! its own startup block through `__xila_current_startup`, never through a global.
//!
//! This module exists only on targets with stack switching (Linux on x86-64 and AArch64,
//! and bare-metal Arm, RISC-V, Xtensa and x86-64). On any other target the `native-linked`
//! feature compiles and adds nothing: a linked program cannot suspend there, so it has no
//! safe way to share an executor with other tasks.

use abi_application::{HomeGrant, NativeEntry, ProcessLaunch, ProcessOwner, start_process};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::num::NonZeroUsize;
use file_system::PathOwned;
use stackful::{DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE, Stack};

use crate::{Home, MainFuture, NetworkRights, Standard};

/// The entry of a linked program: the C ABI of a native entry.
///
/// # Safety
/// The entry obeys the startup rules of `XilaStartup`, fits on the configured stack and
/// returns without unwinding.
pub type LinkedEntry = NativeEntry;

/// Turn an entry whose startup type is another Rust declaration of `XilaStartup` into a
/// [`LinkedEntry`]. The SDK crate `xila-sys` declares its own `XilaStartup`, with the layout
/// of the host's, so its entry cannot coerce to [`LinkedEntry`] directly. Only the pointee
/// type differs: the call is the same C call.
///
/// # Safety
/// `T` has the layout of `abi_application::XilaStartup`, size checked at compile time.
pub const unsafe fn linked_entry<T>(entry: unsafe extern "C" fn(*const T) -> i32) -> LinkedEntry {
    const { assert!(core::mem::size_of::<T>() == core::mem::size_of::<abi_application::XilaStartup>()) };
    // SAFETY: only the pointee type of the argument differs, and the caller vouches for it.
    unsafe { core::mem::transmute(entry) }
}

/// The arguments of a program, named for [`linked_executable!`].
#[doc(hidden)]
pub type LinkedArguments = Vec<String>;

/// The exit code of a program that could not start: no stack, no state or no task.
pub const START_FAILURE: NonZeroUsize = NonZeroUsize::MIN;

/// The directory that a linked program gets as its home capability.
pub enum LinkedHome {
    /// No home capability.
    None,
    /// The home that the account of the task's user records, with read and write rights.
    /// The program gets no home capability if the account has none.
    User,
    /// This directory, with these rights.
    Path(Home),
}

impl LinkedHome {
    /// A directory that the program can read and write.
    pub fn read_write(path: PathOwned) -> Self {
        Self::Path(Home { path, rights: 3 })
    }
}

/// How a linked program starts.
pub struct LinkedOptions {
    /// Usable bytes of the coroutine stack. Taken from the heap while the program runs.
    /// The default is [`DEFAULT_MIN_STACK_SIZE`]: a program that formats text or calls into
    /// a C library needs more.
    pub stack_size: usize,
    /// The network rights of the program. None by default.
    pub network: NetworkRights,
    /// The home directory capability. The user's home by default.
    pub home: LinkedHome,
}

impl Default for LinkedOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkedOptions {
    pub const fn new() -> Self {
        Self {
            stack_size: DEFAULT_MIN_STACK_SIZE,
            network: NetworkRights::NONE,
            home: LinkedHome::User,
        }
    }

    pub const fn stack_size(mut self, stack_size: usize) -> Self {
        self.stack_size = stack_size;
        self
    }

    pub const fn network(mut self, network: NetworkRights) -> Self {
        self.network = network;
        self
    }

    pub fn home(mut self, home: LinkedHome) -> Self {
        self.home = home;
        self
    }
}

/// The exit code that the task reports for an exit status of the entry.
fn exit_code(status: i32) -> core::result::Result<(), NonZeroUsize> {
    // `unsigned_abs` keeps `i32::MIN` from overflowing.
    NonZeroUsize::new(status.unsigned_abs() as usize).map_or(Ok(()), Err)
}

/// Run a linked entry as a native process on the calling task, until it returns or exits.
///
/// The future initializes the services that native programs use (the clock and the entropy
/// source), starts a coroutine on a stack of `options.stack_size` bytes and drives it. The
/// program gets the process capability, the arguments and the standard streams of the
/// caller, the environment that the task inherited from its parent (`TZ` included), the
/// home and the network rights of `options`, and the graphics service if the context has
/// one. Its program name is the name of the task.
///
/// Dropping the future cancels the program, even while it waits on a blocking call: the
/// coroutine stack is freed with no destructor run on it, and the host state closes every
/// handle, stream and graphics object that the program still held.
///
/// An exit status of zero resolves to `Ok(())`. Any other status resolves to an error that
/// holds its absolute value. A program that cannot start resolves to [`START_FAILURE`].
///
/// # Safety contract
/// The caller vouches that `entry` is trusted code that obeys the startup rules, fits on the
/// configured stack and returns without unwinding. Linked code is part of the firmware, so
/// it is not confined.
pub fn run_linked(
    entry: LinkedEntry,
    options: LinkedOptions,
    standard: Standard<'static>,
    arguments: Vec<String>,
) -> MainFuture {
    Box::pin(async move {
        let context = standard.context;
        abi_application::initialize(context.time_manager);
        abi_application::initialize_random(context.virtual_file_system).await;

        let home = match options.home {
            LinkedHome::None => None,
            LinkedHome::Path(home) => Some(home),
            LinkedHome::User => user_home(context).await,
        }
        .map(|home| HomeGrant {
            path: home.path,
            rights: home.rights,
        });

        // Allocated before the state, fallibly: a failed start must leave nothing behind.
        let Some(stack) = Stack::try_with_min_size(options.stack_size, MIN_STACK_SIZE) else {
            log::error!(
                "linked program: no memory for a stack of {} bytes",
                options.stack_size
            );
            return Err(START_FAILURE);
        };
        let (input, output, error) = standard.split();
        let wrap = |file: virtual_file_system::File<'static>| {
            Some(abi_application::File::new(file.into_synchronous_file()))
        };
        let owner =
            match ProcessOwner::from_startup((), arguments, wrap(input), wrap(output), wrap(error))
            {
                Ok(owner) => owner,
                Err(status) => {
                    log::error!("linked program: cannot allocate the process state: {status:?}");
                    return Err(START_FAILURE);
                }
            };
        let launch = ProcessLaunch {
            host: context,
            // The task keeps the environment that it copied from its parent.
            environment: None,
            home,
            network: u32::from(options.network.bits()),
        };
        // SAFETY: the caller of the macro or function vouches for the entry. The future runs
        // on the executor that polls it, which is the one of the task that started it.
        let status = unsafe { start_process(stack, owner, entry, Some(launch)) }.await;
        exit_code(status)
    })
}

/// The user's home as a grant, or `None` if the task, the user or the account has none.
async fn user_home(context: &'static crate::ExecutableContext) -> Option<Home> {
    let manager = context.task_manager;
    let task = manager.try_get_current_task_identifier().await.ok()?;
    let user = manager.get_user(task).await.ok()?;
    let path = context.virtual_file_system.get_user_home(user).await.ok()?;
    Some(Home { path, rights: 3 })
}

/// Declare an executable that runs a linked entry, for `mount_executables!`.
///
/// ```ignore
/// executable::linked_executable!(pub Greeter => greeter_entry);
/// executable::linked_executable!(Worker => worker_entry, stack_size = 32 * 1024,
///     network = NetworkRights::CONNECT);
/// ```
///
/// The entry is an `unsafe extern "C" fn(*const XilaStartup) -> i32`, either from
/// `abi_application` or from `xila-sys` (see [`linked_entry`]). Declaring it vouches that the
/// entry is trusted code that obeys the startup rules. The optional settings
/// are the builder methods of [`LinkedOptions`]: `stack_size`, `network` and `home`.
#[macro_export]
macro_rules! linked_executable {
    ($(#[$attribute:meta])* $visibility:vis $name:ident => $entry:path $(, $setting:ident = $value:expr)* $(,)?) => {
        $(#[$attribute])*
        $visibility struct $name;

        impl $crate::ExecutableTrait for $name {
            fn main(
                standard: $crate::Standard<'static>,
                arguments: $crate::LinkedArguments,
            ) -> $crate::MainFuture {
                $crate::run_linked(
                    // SAFETY: the declaration vouches for the entry, see `linked_executable!`.
                    unsafe { $crate::linked_entry($entry) },
                    $crate::LinkedOptions::new() $(.$setting($value))*,
                    standard,
                    arguments,
                )
            }
        }
    };
}
