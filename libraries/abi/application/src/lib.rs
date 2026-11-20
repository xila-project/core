//! Host implementation of the native application C ABI.
//!
//! Core's native ELF handler installs the time manager from its executable
//! context and discovers the entropy character device at
//! `/devices/random/source` before launch. The first successful installation of
//! each service is retained for the system. Native applications can run
//! without an entropy device, in which case nonempty random fills return
//! [`Status::Unavailable`].
//!
//! A native process is the Core task that runs a native entry on a stackful
//! coroutine. The `XilaStartup.process` capability gives it its identity, its
//! environment (the task's own), spawn and wait for children, and exit, with
//! no table of its own: see [`Process`] and the crate README.

#![no_std]
#![deny(clippy::unwrap_used, clippy::expect_used)]

extern crate alloc;

mod directory;
mod file;
mod iterator;
mod metadata;
#[cfg(feature = "network")]
mod network;
mod network_types;
mod poll;
mod process;
mod startup;
#[cfg(feature = "network")]
pub use network::*;
pub use network_types::*;
mod random;
mod status;
mod threads;
mod time;
pub use threads::*;

pub use directory::*;
pub use file::*;
pub use iterator::*;
pub use metadata::*;
pub use poll::*;
pub use process::*;
pub use random::*;
pub use startup::*;
pub use status::*;
pub use time::*;

/// Compatibility level of the application ABI, as recorded in package
/// manifests (`minimum_xila_abi`) and registry entries. Version 1 is the first
/// level that package manifests declare; unversioned historical builds have no
/// declared compatibility. Increment it for a breaking ABI change.
///
/// It versions the whole application contract, Wasm and native, as seen by
/// the package manager. `XILA_ABI_VERSION_MAJOR`/`MINOR` version only the
/// native `XilaApi` table layout.
///
/// cbindgen:ignore
pub const APPLICATION_ABI_VERSION: u32 = 1;

pub const XILA_ABI_VERSION_MAJOR: u32 = 0;
pub const XILA_ABI_VERSION_MINOR: u32 = 1;

mod api;
mod heap;
pub use api::{XILA_API, XilaApi};
pub use heap::*;

#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
use stackful::{is_in_coroutine, suspend_on, suspend_on_cancellable};

/// End the running coroutine with an exit status. Callers have checked
/// `is_in_coroutine`.
#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
fn exit_coroutine(status: i32) -> ! {
    // SAFETY: `xila_process_exit` released everything the process owns on this stack.
    unsafe { stackful::exit(status) }
}

#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
use task::block_on as suspend_on;

#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
use task::block_on as suspend_on_cancellable;

#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
const fn is_in_coroutine() -> bool {
    false
}

/// Targets without stack switching never reach this: they are never in a coroutine.
#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
fn exit_coroutine(_: i32) -> ! {
    unreachable!("exit outside a coroutine")
}

/// The one shared, non-generic `suspend_on` instantiation for unit-output futures.
/// Sleep and random fill both go through it so the generic code exists once.
#[inline(never)]
fn suspend_dynamic(future: core::pin::Pin<&mut dyn core::future::Future<Output = ()>>) {
    suspend_on(future)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixture setup may report errors through Debug.
mod tests {
    extern crate std;
    use super::*;
    use alloc::{boxed::Box, vec};
    use core::ptr;
    use file_system::{AccessFlags, CreateFlags, StateFlags};
    testing::instantiate_global_allocator!();
    #[test]
    fn generated_header_and_sdk() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = std::path::Path::new(env!("OUT_DIR"));
        for (generated, committed) in [
            ("application.h", "include/xila/application.h"),
            ("stubs.h", "sdk/stubs.h"),
            ("stubs.c", "sdk/stubs.c"),
            // This snapshot pins both the symbol names and the slot order.
            ("slots.tsv", "sdk/slots.tsv"),
        ] {
            let generated = std::fs::read(out.join(generated)).unwrap();
            let committed = root.join(committed);
            if std::env::var_os("XILA_UPDATE_HEADER").is_some() {
                std::fs::write(&committed, &generated).unwrap();
            }
            assert!(
                std::fs::read(&committed).unwrap() == generated,
                "{} is stale; regenerate with XILA_UPDATE_HEADER=1",
                committed.display()
            );
        }
    }

    #[cfg(not(feature = "network"))]
    #[test]
    fn disabled_network_keeps_its_slots() {
        assert_eq!(
            unsafe { (XILA_API.network_close)(ptr::null_mut()) },
            Status::Unsupported
        );
        assert_eq!(XILA_API.size as usize, core::mem::size_of::<XilaApi>());
    }
    #[test]
    fn validation() {
        for path in [
            "/absolute",
            "..",
            "a/../b",
            "x\0y",
            "",
            ".",
            "./",
            "./.",
            "//",
        ] {
            assert_eq!(canonical("/root", path, true), Err(Status::InvalidArgument));
        }
        // Without `entry`, the capability itself may be named (directory open, statistics).
        for path in ["", ".", "./"] {
            assert_eq!(canonical("/root", path, false).unwrap().as_str(), "/root");
        }
        assert_eq!(
            canonical("/root", "./a//./b/", true).unwrap().as_str(),
            "/root/a/b"
        );
        assert_eq!(validate_rights(4), Err(Status::InvalidArgument));
        for flags in [0, 8, 9, 128, u32::MAX, 1 | 16, 1 | 32] {
            assert_eq!(open_flags(flags, 3), Err(Status::InvalidArgument));
        }
        assert_eq!(open_flags(1, 2), Err(Status::PermissionDenied));
        for flags in [2, 5, 2 | 16, 2 | 32] {
            assert_eq!(open_flags(flags, 1), Err(Status::PermissionDenied));
        }
        let flags = open_flags(127, 3).unwrap();
        assert_eq!(flags.get_access(), AccessFlags::READ_WRITE);
        assert_eq!(flags.get_create(), CreateFlags::all());
        assert_eq!(
            flags.get_state(),
            StateFlags::Append | StateFlags::NonBlocking
        );
        let flags = open_flags(XILA_OPEN_READ, 1).unwrap();
        assert_eq!(flags.get_access(), AccessFlags::Read);
        assert_eq!(flags.get_create(), CreateFlags::empty());
        assert_eq!(flags.get_state(), StateFlags::empty());
        unsafe {
            assert_eq!(
                path_input([255u8].as_ptr().cast(), 1),
                Err(Status::InvalidArgument)
            );
        }
    }
    #[test]
    fn filesystem_validation() {
        unsafe {
            let mut stats = Statistics {
                size: 72,
                kind: 99,
                ..Statistics::default()
            };
            assert_eq!(
                xila_file_get_statistics(ptr::null_mut(), &mut stats),
                Status::InvalidArgument
            );
            assert_eq!(stats.kind, 99);
            assert_eq!(
                xila_file_set_size(ptr::null_mut(), 0),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_get_statistics(ptr::null(), c"".as_ptr(), 0, 0, &mut stats),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_create(ptr::null(), c"a".as_ptr(), 1),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_remove(ptr::null(), c"a".as_ptr(), 1, 0),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_rename(ptr::null(), c"a".as_ptr(), 1, ptr::null(), c"b".as_ptr(), 1),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_iterator_open(ptr::null(), c"".as_ptr(), 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_iterator_next(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_iterator_rewind(ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_directory_iterator_close(ptr::null_mut()), Status::Ok);
            assert_eq!(crate::metadata::validate_output(&mut stats), Ok(()));
            stats.size = 71;
            assert_eq!(
                crate::metadata::validate_output(&mut stats),
                Err(Status::InvalidArgument)
            );
        }
        assert_eq!(core::mem::size_of::<Statistics>(), 72);
        assert_eq!(core::mem::offset_of!(Statistics, file_size), 24);
        assert_eq!(core::mem::offset_of!(Statistics, access_nanoseconds), 48);
        std::println!(
            "File={} Directory={} DirectoryIterator={} Statistics={}",
            core::mem::size_of::<File>(),
            core::mem::size_of::<Directory>(),
            core::mem::size_of::<DirectoryIterator>(),
            core::mem::size_of::<Statistics>()
        );
    }
    #[test]
    fn kind_values_are_explicit() {
        use file_system::Kind;
        for (kind, value) in [
            (Kind::File, XILA_KIND_FILE),
            (Kind::Directory, XILA_KIND_DIRECTORY),
            (Kind::BlockDevice, XILA_KIND_BLOCK_DEVICE),
            (Kind::CharacterDevice, XILA_KIND_CHARACTER_DEVICE),
            (Kind::Pipe, XILA_KIND_PIPE),
            (Kind::Socket, XILA_KIND_SOCKET),
            (Kind::SymbolicLink, XILA_KIND_SYMLINK),
        ] {
            assert_eq!(kind_value(kind), value);
        }
    }
    #[test]
    fn resolver_errors_report_the_status_of_their_kind() {
        use file_system::io_error::ResolveError;
        assert_eq!(
            Status::from(ResolveError::NoAnswer),
            Status::NameNotResolved
        );
        assert_eq!(Status::from(ResolveError::Failure), Status::ResolverFailure);
        assert_eq!(
            Status::from(ResolveError::InvalidName),
            Status::InvalidDnsName
        );
        assert_eq!(Status::from(ResolveError::NoInterface), Status::Unavailable);
        for &error in ResolveError::ALL {
            assert_eq!(Status::from(error) as u32, u32::from(error.id()));
            assert_eq!(Status::from(error), Status::from(error.kind()));
        }
    }

    #[test]
    fn status_and_kind_match_frozen_wire_table() {
        use file_system::IoErrorKind as Kind;
        let rows = [
            (Kind::InvalidArgument, Status::InvalidArgument, 0x0101),
            (Kind::InvalidData, Status::InvalidData, 0x0102),
            (Kind::PermissionDenied, Status::PermissionDenied, 0x0103),
            (
                Kind::OperationNotPermitted,
                Status::OperationNotPermitted,
                0x0104,
            ),
            (Kind::InputOutput, Status::InputOutput, 0x0105),
            (Kind::OutOfMemory, Status::OutOfMemory, 0x0106),
            (Kind::WouldBlock, Status::WouldBlock, 0x0107),
            (Kind::ResourceBusy, Status::ResourceBusy, 0x0108),
            (Kind::Interrupted, Status::Interrupted, 0x0109),
            (Kind::TimedOut, Status::TimedOut, 0x010A),
            (Kind::Unsupported, Status::Unsupported, 0x010B),
            (Kind::Overflow, Status::Overflow, 0x010C),
            (Kind::WriteZero, Status::WriteZero, 0x010D),
            (Kind::NoBuffers, Status::NoBuffers, 0x010E),
            (Kind::NotFound, Status::NotFound, 0x010F),
            (Kind::AlreadyExists, Status::AlreadyExists, 0x0110),
            (Kind::InvalidFlags, Status::InvalidFlags, 0x0111),
            (Kind::InvalidState, Status::InvalidState, 0x0112),
            (Kind::NotImplemented, Status::NotImplemented, 0x0113),
            (Kind::ExecFormat, Status::ExecFormat, 0x0114),
            (Kind::NoChild, Status::NoChild, 0x0115),
            (
                Kind::ArgumentListTooLong,
                Status::ArgumentListTooLong,
                0x0116,
            ),
            (Kind::Deadlock, Status::Deadlock, 0x0117),
            (Kind::BadDescriptor, Status::BadDescriptor, 0x0201),
            (Kind::InvalidPath, Status::InvalidPath, 0x0202),
            (
                Kind::MissingPathComponent,
                Status::MissingPathComponent,
                0x0203,
            ),
            (Kind::MissingFinalEntry, Status::MissingFinalEntry, 0x0204),
            (Kind::NameTooLong, Status::NameTooLong, 0x0205),
            (Kind::SymlinkLoop, Status::SymlinkLoop, 0x0206),
            (Kind::NotADirectory, Status::NotADirectory, 0x0207),
            (Kind::IsADirectory, Status::IsADirectory, 0x0208),
            (Kind::DirectoryNotEmpty, Status::DirectoryNotEmpty, 0x0209),
            (Kind::NoSpace, Status::NoSpace, 0x020A),
            (Kind::FileTooLarge, Status::FileTooLarge, 0x020B),
            (Kind::ProcessFileLimit, Status::ProcessFileLimit, 0x020C),
            (Kind::SystemFileLimit, Status::SystemFileLimit, 0x020D),
            (Kind::CrossDevice, Status::CrossDevice, 0x020E),
            (Kind::PathEscapesRoot, Status::PathEscapesRoot, 0x020F),
            (Kind::CorruptData, Status::CorruptData, 0x0210),
            (Kind::MissingAttribute, Status::MissingAttribute, 0x0211),
            (Kind::StaleHandle, Status::StaleHandle, 0x0212),
            (Kind::ReadOnlyFileSystem, Status::ReadOnlyFileSystem, 0x0213),
            (Kind::InvalidSeek, Status::InvalidSeek, 0x0214),
            (Kind::DeviceUnavailable, Status::Unavailable, 0x0301),
            (Kind::NoSuchTask, Status::NoSuchTask, 0x0302),
            (
                Kind::InappropriateIoControl,
                Status::InappropriateIoControl,
                0x0303,
            ),
            (Kind::ConnectionRefused, Status::ConnectionRefused, 0x0401),
            (Kind::ConnectionReset, Status::ConnectionReset, 0x0402),
            (Kind::ConnectionAborted, Status::ConnectionAborted, 0x0403),
            (Kind::NotConnected, Status::NotConnected, 0x0404),
            (Kind::AlreadyConnected, Status::AlreadyConnected, 0x0405),
            (Kind::AddressInUse, Status::AddressInUse, 0x0406),
            (
                Kind::AddressNotAvailable,
                Status::AddressNotAvailable,
                0x0407,
            ),
            (
                Kind::DestinationRequired,
                Status::DestinationRequired,
                0x0408,
            ),
            (Kind::HostUnreachable, Status::HostUnreachable, 0x0409),
            (Kind::NetworkUnreachable, Status::NetworkUnreachable, 0x040A),
            (Kind::NetworkDown, Status::NetworkDown, 0x040B),
            (Kind::BrokenPipe, Status::BrokenPipe, 0x040C),
            (Kind::MessageTooLarge, Status::MessageTooLarge, 0x040D),
            (Kind::DatagramTruncated, Status::DatagramTruncated, 0x040E),
            (
                Kind::SocketOperationUnsupported,
                Status::SocketOperationUnsupported,
                0x040F,
            ),
            (
                Kind::ProtocolUnsupported,
                Status::ProtocolUnsupported,
                0x0410,
            ),
            (
                Kind::AddressFamilyUnsupported,
                Status::AddressFamilyUnsupported,
                0x0411,
            ),
            (Kind::InProgress, Status::InProgress, 0x0412),
            (Kind::AlreadyInProgress, Status::AlreadyInProgress, 0x0413),
            (Kind::ProtocolError, Status::ProtocolError, 0x0414),
            (Kind::NotASocket, Status::NotASocket, 0x0415),
            (Kind::InvalidDnsName, Status::InvalidDnsName, 0x0501),
            (
                Kind::UnsupportedDnsQuery,
                Status::UnsupportedDnsQuery,
                0x0502,
            ),
            (Kind::NameNotResolved, Status::NameNotResolved, 0x0503),
            (
                Kind::ResolverTemporaryFailure,
                Status::ResolverTemporaryFailure,
                0x0504,
            ),
            (Kind::ResolverFailure, Status::ResolverFailure, 0x0505),
        ];
        assert_eq!(rows.len(), Kind::ALL.len());
        for (kind, status, id) in rows {
            assert_eq!(Status::from(kind), status);
            assert_eq!(status.io_kind(), Some(kind));
            assert_eq!(status as u32, id);
        }
        for (status, id) in [
            (Status::Ok, 0),
            (Status::End, 0x0701),
            (Status::BufferTooSmall, 0x0702),
        ] {
            assert_eq!(status.io_kind(), None);
            assert_eq!(status as u32, id);
        }
    }
    #[test]
    fn nulls_and_uninitialized_time() {
        unsafe {
            assert_eq!(
                xila_file_open(ptr::null(), ptr::null(), 0, 1, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_open(ptr::null(), ptr::null(), 0, 1, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_read(ptr::null_mut(), ptr::null_mut(), 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_write(ptr::null_mut(), ptr::null(), 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_seek(ptr::null_mut(), 0, 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_file_flush(ptr::null_mut()), Status::InvalidArgument);
            assert_eq!(xila_file_close(ptr::null_mut()), Status::Ok);
            xila_directory_close(ptr::null_mut());
            let mut mono = 123;
            let mut real = 456;
            assert_eq!(xila_time_monotonic(&mut mono), Status::Unavailable);
            assert_eq!(xila_time_realtime(&mut real), Status::Unavailable);
            assert_eq!((mono, real), (123, 456));
            assert_eq!(
                xila_time_monotonic(ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_time_realtime(ptr::null_mut()), Status::InvalidArgument);
            // No clock is installed: resolution is unavailable, bad arguments still
            // fail first, and the output is untouched.
            let mut resolution = 789;
            for clock in [XILA_CLOCK_MONOTONIC, XILA_CLOCK_REALTIME] {
                assert_eq!(
                    xila_time_resolution(clock, &mut resolution),
                    Status::Unavailable
                );
                assert_eq!(
                    xila_time_resolution(clock, ptr::null_mut()),
                    Status::InvalidArgument
                );
            }
            assert_eq!(
                xila_time_resolution(2, &mut resolution),
                Status::InvalidArgument
            );
            assert_eq!(resolution, 789);
            // Sleeping needs the executor's timer, not the manager.
            assert_eq!(xila_time_sleep(0), Status::Ok);
            let clock = &::time::EmbassyClock;
            let start = ::time::MonotonicClock::now(clock);
            assert_eq!(xila_time_sleep(2_000_000), Status::Ok);
            let elapsed = ::time::MonotonicClock::now(clock) - start;
            assert!(elapsed >= core::time::Duration::from_millis(2));
            // No entropy source is installed: only an empty fill can succeed.
            let mut bytes = [0x5A_u8; 4];
            assert_eq!(xila_random_fill(ptr::null_mut(), 0), Status::Ok);
            assert_eq!(xila_random_fill(bytes.as_mut_ptr().cast(), 0), Status::Ok);
            assert_eq!(
                xila_random_fill(ptr::null_mut(), 1),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_random_fill(bytes.as_mut_ptr().cast(), isize::MAX as usize + 1),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_random_fill(bytes.as_mut_ptr().cast(), bytes.len()),
                Status::Unavailable
            );
            assert_eq!(bytes, [0x5A; 4]);
        }
        std::println!(
            "File={} Directory={}",
            core::mem::size_of::<File>(),
            core::mem::size_of::<Directory>()
        );
    }

    /// A host that never starts a child: process identity and environment need no more.
    struct NoSpawnHost(&'static task::Manager);
    impl ProcessHost for NoSpawnHost {
        fn task_manager(&self) -> &'static task::Manager {
            self.0
        }
        fn virtual_file_system(&self) -> &'static virtual_file_system::VirtualFileSystem {
            unreachable!("no home directory is granted")
        }
        fn spawn(&'static self, _: SpawnRequest) -> SpawnFuture {
            Box::pin(async { Err(Status::Unsupported) })
        }
    }

    std::thread_local! {
        static EXPECTED: core::cell::Cell<(u32, u32)> = const { core::cell::Cell::new((0, 0)) };
    }

    macro_rules! check {
        ($condition:expr) => {
            if !$condition {
                return line!() as i32;
            }
        };
    }

    /// Runs on a plain task, not a coroutine: calls that cannot suspend must say so.
    unsafe extern "C" fn probe_entry(startup: *const XilaStartup) -> i32 {
        let startup = unsafe { &*startup };
        let api = unsafe { &*startup.api };
        let process = startup.process;
        check!(!process.is_null() && startup.home.is_null());
        let (mut id, mut parent) = (u32::MAX, u32::MAX);
        let mut length = 99;
        let mut buffer = [b'#'; 8];
        unsafe {
            check!((api.process_id)(process, &mut id) == Status::Ok);
            check!((api.process_parent_id)(process, &mut parent) == Status::Ok);
            check!((id, parent) == EXPECTED.get());
            // Only a process running on its own coroutine can exit.
            check!((api.process_exit)(process, 3) == Status::Unsupported);
            check!((api.process_exit)(ptr::null(), 3) == Status::InvalidArgument);
            // A missing variable leaves the length untouched.
            check!(
                (api.env_get)(
                    process,
                    c"K".as_ptr(),
                    1,
                    buffer.as_mut_ptr().cast(),
                    8,
                    &mut length
                ) == Status::NotFound
            );
            check!(length == 99);
            check!((api.env_set)(process, c"K".as_ptr(), 1, c"value".as_ptr(), 5) == Status::Ok);
            check!(
                (api.env_get)(
                    process,
                    c"K".as_ptr(),
                    1,
                    buffer.as_mut_ptr().cast(),
                    8,
                    &mut length
                ) == Status::Ok
            );
            check!(length == 5 && &buffer == b"value###");
            check!(
                (api.env_get)(
                    process,
                    c"K".as_ptr(),
                    1,
                    buffer.as_mut_ptr().cast(),
                    4,
                    &mut length
                ) == Status::BufferTooSmall
            );
            check!(length == 5 && &buffer == b"value###");
            // Invalid names and values.
            for name in [&b""[..], b"A=B", b"A\0B", b"\xff"] {
                check!(
                    (api.env_set)(process, name.as_ptr().cast(), name.len(), c"v".as_ptr(), 1)
                        == Status::InvalidArgument
                );
                check!(
                    (api.env_get)(
                        process,
                        name.as_ptr().cast(),
                        name.len(),
                        ptr::null_mut(),
                        0,
                        &mut length
                    ) == Status::InvalidArgument
                );
                check!(
                    (api.env_remove)(process, name.as_ptr().cast(), name.len())
                        == Status::InvalidArgument
                );
            }
            check!(
                (api.env_set)(process, c"K".as_ptr(), 1, c"a".as_ptr(), usize::MAX)
                    == Status::InvalidArgument
            );
            check!(
                (api.env_set)(process, c"K".as_ptr(), 1, ptr::null(), 1) == Status::InvalidArgument
            );
            check!((api.env_set)(process, c"K".as_ptr(), 1, ptr::null(), 0) == Status::Ok);
            check!(
                (api.env_get)(process, c"K".as_ptr(), 1, ptr::null_mut(), 0, &mut length)
                    == Status::Ok
            );
            check!(length == 0);
            check!((api.env_remove)(process, c"K".as_ptr(), 1) == Status::Ok);
            check!((api.env_remove)(process, c"K".as_ptr(), 1) == Status::Ok);
            // Wait needs a child this process spawned.
            check!(
                (api.process_wait)(process, ptr::null_mut(), 0, ptr::null_mut())
                    == Status::InvalidArgument
            );
            check!((api.process_child_close)(ptr::null_mut()) == Status::Ok);
        }
        0
    }

    unsafe extern "C" fn quiet_entry(startup: *const XilaStartup) -> i32 {
        let startup = unsafe { &*startup };
        let api = unsafe { &*startup.api };
        let process = startup.process;
        if !process.is_null() {
            let mut id = 0;
            let mut length = 0;
            unsafe {
                check!((api.process_id)(process, &mut id) == Status::Ok);
                check!((api.process_parent_id)(process, &mut id) == Status::Ok);
                check!(
                    (api.env_get)(
                        process,
                        c"NAME".as_ptr(),
                        4,
                        ptr::null_mut(),
                        0,
                        &mut length
                    ) == Status::NotFound
                );
                check!((api.env_remove)(process, c"NAME".as_ptr(), 4) == Status::Ok);
            }
        }
        0
    }

    #[task::test]
    async fn process_identity_and_environment_without_a_coroutine() {
        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let host: &'static NoSpawnHost = Box::leak(Box::new(NoSpawnHost(tasks)));
        EXPECTED.set((
            identifier(task),
            identifier(tasks.get_parent(task).await.unwrap()),
        ));
        let mut heap = Heap::new();
        let status = unsafe {
            enter(
                probe_entry,
                &mut heap,
                Some((host, task)),
                Resources::default(),
            )
        };
        assert_eq!(status, 0, "line of the failed check");
        // A task that left no variables behind.
        assert!(
            tasks
                .get_environment_variables(task)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// An empty environment and the process capability itself take no allocation:
    /// the capability lives in the entry call's frame, and the environment is the
    /// task's own, empty until something sets a variable.
    #[task::test]
    async fn process_capability_and_empty_environment_allocate_nothing() {
        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let host: &'static NoSpawnHost = Box::leak(Box::new(NoSpawnHost(tasks)));
        let mut heap = Heap::new();
        let before = testing::MemoryManager::thread_allocations();
        let status_with = unsafe {
            enter(
                quiet_entry,
                &mut heap,
                Some((host, task)),
                Resources::default(),
            )
        };
        let with = testing::MemoryManager::thread_allocations().since(before);
        let before = testing::MemoryManager::thread_allocations();
        let status_without = unsafe { enter(quiet_entry, &mut heap, None, Resources::default()) };
        let without = testing::MemoryManager::thread_allocations().since(before);
        assert_eq!((status_with, status_without), (0, 0));
        assert_eq!(with.allocations(), 0);
        assert_eq!(without.allocations(), 0);
    }

    unsafe extern "C" fn return_entry(startup: *const XilaStartup) -> i32 {
        let startup = unsafe { &*startup };
        let api = unsafe { &*startup.api };
        check!(startup.argument_count == 1);
        unsafe {
            check!(
                (api.env_set)(startup.process, c"NAME".as_ptr(), 4, c"v".as_ptr(), 1) == Status::Ok
            )
        };
        4
    }

    unsafe extern "C" fn exit_entry(startup: *const XilaStartup) -> i32 {
        let startup = unsafe { &*startup };
        let api = unsafe { &*startup.api };
        check!(startup.argument_count == 1);
        unsafe {
            check!(
                (api.env_set)(startup.process, c"NAME".as_ptr(), 4, c"v".as_ptr(), 1) == Status::Ok
            );
            let _ = (api.process_exit)(startup.process, 5);
        }
        1 // Exit returned.
    }

    /// Leaving through `xila_process_exit` frees exactly what returning does: the
    /// startup strings, the explicit environment and the coroutine stack.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn exit_and_return_release_the_same_memory() {
        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let host: &'static NoSpawnHost = Box::leak(Box::new(NoSpawnHost(tasks)));
        let run = async |entry: unsafe extern "C" fn(*const XilaStartup) -> i32| {
            let before = testing::MemoryManager::thread_allocations();
            let heap = Box::into_raw(Box::new(Heap::new()));
            let launch = ProcessLaunch {
                host,
                environment: Some(vec![
                    task::EnvironmentVariable::new("A", "1"),
                    task::EnvironmentVariable::new("B", "2"),
                ]),
                home: None,
            };
            // SAFETY: the bounded entries fit the stack and never unwind, and the closure
            // moves everything it owns into `run`.
            let (handle, _) = unsafe {
                stackful::spawn_with_result(
                    tasks,
                    task,
                    "process",
                    stackful::Stack::new(32 * 1024),
                    move || {
                        launch.run(entry, &mut *heap, vec!["argument".into()], None, None, None)
                    },
                    |status| status as isize,
                )
                .await
                .unwrap()
            };
            let status = handle.join().await;
            task::yield_now().await;
            drop(unsafe { Box::from_raw(heap) });
            let delta = testing::MemoryManager::thread_allocations().since(before);
            (status, delta.net_bytes())
        };
        // The first runs grow the task manager's retained pool.
        run(return_entry).await;
        run(exit_entry).await;
        let (returned, returned_bytes) = run(return_entry).await;
        let (exited, exited_bytes) = run(exit_entry).await;
        assert_eq!((returned, exited), (4, 5));
        assert_eq!(returned_bytes, 0);
        assert_eq!(exited_bytes, returned_bytes);
    }
}
