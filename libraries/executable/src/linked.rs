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
//! Every launch of a C program starts from pristine SDK state: `xila_linked_start` resets the
//! whole SDK/POSIX runtime (descriptors, standard streams and their locks and buffers, the
//! environment, the exit handlers, the locks, ...) before `main` runs, whichever way the
//! previous launch of the same program ended, including a cancellation, whose process heap
//! the host already freed. The program's own C `static` variables persist across launches,
//! though: initialize them at the top of `main` if a relaunch must not see the previous
//! run's values.
//!
//! A C program localized with `llvm-objcopy` (see the SDK's `link-archive` command) gets
//! private `.data`/`.bss` per program, so two different programs never collide. Two
//! concurrent launches of the *same* C program, however, share that program's globals
//! (`xila_sdk_api`, `xila_process`, `stdout`, ...), so such a program runs exclusively:
//! [`run_linked_exclusive`] runs one instance at a time and fails a second concurrent
//! launch with [`BUSY_FAILURE`].
//!
//! This module exists only on targets with stack switching (Linux on x86-64 and AArch64,
//! and bare-metal Arm, RISC-V, Xtensa and x86-64). On any other target the `native-linked`
//! feature compiles and adds nothing: a linked program cannot suspend there, so it has no
//! safe way to share an executor with other tasks.

use abi_application::{HomeGrant, NativeEntry, ProcessLaunch, ProcessOwner, start_process};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::num::NonZeroUsize;
use file_system::PathOwned;
use portable_atomic::Ordering;
use stackful::{DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE, Stack};

/// The flag type of an [`ExclusiveSlot`], re-exported so a declaration site (and the
/// `exclusive` form of `linked_executable!`) can name it.
pub use portable_atomic::AtomicBool;

use crate::{Home, MainFuture, NetworkRights, Standard};

/// The rights of a home directory that the program can read and write: the ABI's
/// `XILA_RIGHT_READ | XILA_RIGHT_WRITE`.
const READ_WRITE_RIGHTS: u8 =
    (abi_application::XILA_RIGHT_READ | abi_application::XILA_RIGHT_WRITE) as u8;

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

/// The arguments of a program, named for `linked_executable!`.
#[doc(hidden)]
pub type LinkedArguments = Vec<String>;

/// The exit code of a program that could not start: no stack, no state or no task.
pub const START_FAILURE: NonZeroUsize = NonZeroUsize::MIN;

/// The exit code of an exclusive program that could not start because another instance of
/// the same program is already running: 16, `EBUSY`.
pub const BUSY_FAILURE: NonZeroUsize = NonZeroUsize::new(16).unwrap();

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
        Self::Path(Home {
            path,
            rights: READ_WRITE_RIGHTS,
        })
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
/// # Safety
/// The caller vouches that `entry` is trusted code that obeys the startup rules of
/// `XilaStartup`, fits on the configured stack and returns without unwinding. Linked code is
/// part of the firmware, so it is not confined.
pub unsafe fn run_linked(
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
        .map(|home| HomeGrant::Path {
            path: home.path,
            rights: home.rights,
        });

        let Some((stack, owner, launch)) = linked_setup(
            options.stack_size,
            options.network,
            standard,
            arguments,
            home,
        ) else {
            return Err(START_FAILURE);
        };
        // SAFETY: the caller vouches for the entry. The future runs on the executor that
        // polls it, which is the one of the task that started it.
        let status = unsafe { start_process(stack, owner, entry, Some(launch)) }.await;
        exit_code(status)
    })
}

/// The single-instance slot of an exclusive linked program: one static [`AtomicBool`] per
/// program, shared by every launch of that program.
///
/// A C program localized with `llvm-objcopy` gets private `.data`/`.bss` per program, so
/// two different programs never collide; two concurrent launches of the same program,
/// however, share that program's globals. An exclusive program therefore runs one instance
/// at a time: the slot is taken before the entry runs and released when the run ends.
pub struct ExclusiveSlot {
    active: &'static AtomicBool,
}

impl ExclusiveSlot {
    /// A slot over a static flag: `false` means no instance is running.
    pub const fn new(active: &'static AtomicBool) -> Self {
        Self { active }
    }

    /// Take the slot if it is free, else return `None`: the caller must not touch the entry
    /// or its globals when the slot is busy.
    pub fn try_acquire(&self) -> Option<ExclusiveGuard> {
        self.active
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| ExclusiveGuard {
                active: self.active,
            })
    }
}

/// Holds an [`ExclusiveSlot`] until it is dropped. Dropping releases the slot with
/// `store(false, Ordering::Release)`, so a normal return and a cancellation (the run future
/// dropped while the program is suspended mid-call) both free it.
pub struct ExclusiveGuard {
    active: &'static AtomicBool,
}

impl Drop for ExclusiveGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

/// Run an exclusive linked entry as a native process: like [`run_linked`], but only one
/// instance of the program runs at a time.
///
/// The slot is taken on the first poll of the returned future, right before the entry runs:
/// a future that is never polled holds nothing, and if another instance of the same program
/// already holds the slot, the future resolves to [`BUSY_FAILURE`] without touching the
/// entry or its globals. The slot is released when the program returns or exits, and also
/// when the returned future is dropped (cancelled) while the program is suspended, so the
/// next launch can start. Every launch starts from pristine SDK state (the module docs
/// describe the reset `xila_linked_start` performs even after a cancellation), so a relaunch
/// after a cancelled run is safe.
///
/// # Safety
/// The same contract as [`run_linked`]: the caller vouches that `entry` is trusted code
/// that obeys the startup rules of `XilaStartup`, fits on the configured stack and returns
/// without unwinding. In addition, every launch of `entry` must go through the same `slot`:
/// two different [`ExclusiveSlot`]s for one entry (for example two `exclusive`
/// declarations of the same entry) would let two instances run concurrently and share the
/// program's globals.
pub unsafe fn run_linked_exclusive(
    slot: &'static AtomicBool,
    entry: LinkedEntry,
    options: LinkedOptions,
    standard: Standard<'static>,
    arguments: Vec<String>,
) -> MainFuture {
    Box::pin(async move {
        // Taken on the first poll, right before the run: a busy slot fails the launch
        // immediately, without touching the entry or its globals. The guard moves into the
        // future's state, so dropping the future (cancelling the run mid-call) releases the
        // slot too.
        let Some(guard) = ExclusiveSlot::new(slot).try_acquire() else {
            return Err(BUSY_FAILURE);
        };
        // SAFETY: the caller vouches for the entry, as for `run_linked`.
        let result = unsafe { run_linked(entry, options, standard, arguments) }.await;
        drop(guard);
        result
    })
}

/// The synchronous setup of a linked program: the coroutine stack, the process owner and
/// the launch request. Fallible, and a failed start leaves nothing behind. Kept out of the
/// future and never inlined into it, so the state machine holds neither the stack nor the
/// owner setup, and one copy of this code serves every linked program.
#[inline(never)]
fn linked_setup(
    stack_size: usize,
    network: NetworkRights,
    standard: Standard<'static>,
    arguments: Vec<String>,
    home: Option<HomeGrant>,
) -> Option<(Stack, ProcessOwner<()>, ProcessLaunch)> {
    // Allocated before the state, fallibly: a failed start must leave nothing behind.
    let Some(stack) = Stack::try_with_min_size(stack_size, MIN_STACK_SIZE) else {
        log::error!("linked program: no memory for a stack of {stack_size} bytes");
        return None;
    };
    let host = standard.context;
    let (input, output, error) = standard.split();
    let wrap = |file: virtual_file_system::File<'static>| {
        Some(abi_application::File::new(file.into_synchronous_file()))
    };
    let Ok(owner) =
        ProcessOwner::from_startup((), arguments, wrap(input), wrap(output), wrap(error))
    else {
        log::error!("linked program: cannot allocate the process state");
        return None;
    };
    let launch = ProcessLaunch {
        host,
        // The task keeps the environment that it copied from its parent.
        environment: None,
        home,
        network: u32::from(network.bits()),
    };
    Some((stack, owner, launch))
}

/// The user's home as a grant, or `None` if the task, the user or the account has none.
async fn user_home(context: &'static crate::ExecutableContext) -> Option<Home> {
    let manager = context.task_manager;
    let task = manager.try_get_current_task_identifier().await.ok()?;
    let user = manager.get_user(task).await.ok()?;
    let path = context.virtual_file_system.get_user_home(user).await.ok()?;
    Some(Home {
        path,
        rights: READ_WRITE_RIGHTS,
    })
}

/// Declare an executable that runs a linked entry, for `mount_executables!`. The `unsafe`
/// token is required: it vouches that the entry is trusted code, because calling an
/// arbitrary C-ABI entry is unsafe.
///
/// ```ignore
/// executable::linked_executable!(unsafe pub Greeter => greeter_entry);
/// executable::linked_executable!(unsafe Worker => worker_entry, stack_size = 32 * 1024,
///     network = NetworkRights::CONNECT);
/// ```
///
/// With the `exclusive` token, one instance of the program runs at a time: a second
/// concurrent launch fails immediately with [`BUSY_FAILURE`] instead of sharing the
/// program's globals. Use it for a C program linked with the SDK's `link-archive` command:
///
/// ```ignore
/// executable::linked_executable!(unsafe pub MyCApp => my_c_app_entry, exclusive);
/// executable::linked_executable!(unsafe pub MyCApp => my_c_app_entry, exclusive,
///     stack_size = 32 * 1024);
/// ```
///
/// Declare the `exclusive` form at most once per entry: every launch of a given entry must
/// share the same [`ExclusiveSlot`], and two `exclusive` declarations for one entry would
/// give each its own slot, letting two instances run concurrently. Each launch starts from
/// pristine SDK state (`xila_linked_start` resets the SDK/POSIX runtime even after a
/// cancellation); the program's own C `static` variables persist across launches, so
/// initialize them at the top of `main` if a relaunch must not see the previous run's
/// values.
///
/// The entry is an `unsafe extern "C" fn(*const XilaStartup) -> i32`, either from
/// `abi_application` or from `xila-sys` (see [`linked_entry`]). The optional settings
/// are the builder methods of [`LinkedOptions`]: `stack_size`, `network` and `home`.
#[macro_export]
macro_rules! linked_executable {
    ($(#[$attribute:meta])* unsafe $visibility:vis $name:ident => $entry:path $(, $setting:ident = $value:expr)* $(,)?) => {
        $(#[$attribute])*
        $visibility struct $name;

        impl $crate::ExecutableTrait for $name {
            fn main(
                standard: $crate::Standard<'static>,
                arguments: $crate::LinkedArguments,
            ) -> $crate::MainFuture {
                // SAFETY: the `unsafe` token at the declaration vouches for the entry: it is
                // trusted code that obeys the startup rules, fits on the configured stack
                // and returns without unwinding.
                unsafe {
                    $crate::run_linked(
                        $crate::linked_entry($entry),
                        $crate::LinkedOptions::new() $(.$setting($value))*,
                        standard,
                        arguments,
                    )
                }
            }
        }
    };
    ($(#[$attribute:meta])* unsafe $visibility:vis $name:ident => $entry:path, exclusive $(, $setting:ident = $value:expr)* $(,)?) => {
        $(#[$attribute])*
        $visibility struct $name;

        // The slot lives in an anonymous constant, so two `exclusive` declarations in the
        // same module get one static each. Declare the form once per entry: every launch of
        // an entry must share one slot, or two instances could run concurrently.
        const _: () = {
            /// Held from the start of a run until the run returns or is cancelled: one
            /// instance of this program runs at a time.
            static ACTIVE: $crate::AtomicBool = $crate::AtomicBool::new(false);

            impl $crate::ExecutableTrait for $name {
                fn main(
                    standard: $crate::Standard<'static>,
                    arguments: $crate::LinkedArguments,
                ) -> $crate::MainFuture {
                    // SAFETY: the `unsafe` token at the declaration vouches for the entry:
                    // it is trusted code that obeys the startup rules, fits on the
                    // configured stack and returns without unwinding.
                    unsafe {
                        $crate::run_linked_exclusive(
                            &ACTIVE,
                            $crate::linked_entry($entry),
                            $crate::LinkedOptions::new() $(.$setting($value))*,
                            standard,
                            arguments,
                        )
                    }
                }
            }
        };
    };
}
