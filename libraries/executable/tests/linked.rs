//! Statically linked native programs (`native-linked`): C-ABI and Rust (`xila-sys`) entries
//! mounted as executables, run concurrently on one executor, and cancelled while suspended.
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]

use core::{ffi::c_void, ptr, task::Poll};
use portable_atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{future::poll_fn, sync::Mutex};

use abi_application::{File as NativeFile, XilaStartup};
use executable::{
    BUSY_FAILURE, ExecutableContext, Launch, LinkedHome, LinkedOptions, START_FAILURE, Standard,
    execute, execute_with, linked_executable, mount_executables, run_linked, run_linked_exclusive,
};
use file_system::{
    AccessFlags, CreateFlags, DirectBaseOperations, DirectCharacterDevice, Flags, MountOperations,
    PathOwned, Permissions, Size, StateFlags,
};
use task::{EnvironmentVariable, TaskIdentifier};
use testing_support::MemoryManager;
use virtual_file_system::{File, VirtualFileSystem};

testing_support::instantiate_global_allocator!();

/// Debug builds of the ABI call chain and of the formatting code need room.
const STACK: usize = 256 * 1024;

// ---------------------------------------------------------------------------------------
// Entries written against the C ABI, as a C compiler would see it: the host's own types and
// exported functions, no Rust SDK.
// ---------------------------------------------------------------------------------------

unsafe fn say(file: *mut NativeFile, text: &str) {
    let mut count = 0;
    unsafe {
        abi_application::xila_file_write(
            file,
            text.as_ptr().cast::<c_void>(),
            text.len(),
            &mut count,
        )
    };
}

unsafe fn argument<'a>(startup: &XilaStartup, index: usize) -> &'a str {
    let argument = unsafe { &*startup.arguments.add(index) };
    unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            argument.data.cast(),
            argument.length,
        ))
    }
}

unsafe fn environment(startup: &XilaStartup, name: &str) -> String {
    let mut buffer = [0 as core::ffi::c_char; 64];
    let mut length = 0;
    let status = unsafe {
        abi_application::xila_env_get(
            startup.process,
            name.as_ptr().cast(),
            name.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut length,
        )
    };
    if status != abi_application::Status::Ok {
        return format!("<{status:?}>");
    }
    let bytes = unsafe { core::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Open `name` below the home capability.
unsafe fn open_home(startup: &XilaStartup, name: &str, flags: u32) -> *mut NativeFile {
    let mut file = ptr::null_mut();
    let status = unsafe {
        abi_application::xila_file_open(
            startup.home,
            name.as_ptr().cast(),
            name.len(),
            flags,
            &mut file,
        )
    };
    if status == abi_application::Status::Ok {
        file
    } else {
        ptr::null_mut()
    }
}

/// Reports what the host gave it, then ends as the first argument says.
unsafe extern "C" fn c_report(startup: *const XilaStartup) -> i32 {
    let startup = unsafe { &*startup };
    let out = startup.standard_output;
    let arguments: Vec<&str> = (0..startup.argument_count)
        .map(|index| unsafe { argument(startup, index) })
        .collect();
    let name = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            startup.program_name.data.cast(),
            startup.program_name.length,
        ))
    };
    let found = abi_application::__xila_current_startup();
    unsafe {
        say(out, &format!("name={name}\n"));
        say(out, &format!("arguments={}\n", arguments.join(",")));
        say(out, &format!("same_startup={}\n", ptr::eq(found, startup)));
        say(
            out,
            &format!("host={}\n", environment(startup, "FROM_HOST")),
        );
        say(out, &format!("zone={}\n", environment(startup, "TZ")));
        say(startup.standard_error, "to stderr\n");

        // Write a file in the home directory, then read it back.
        let note = open_home(
            startup,
            "note.txt",
            abi_application::XILA_OPEN_WRITE | abi_application::XILA_OPEN_CREATE,
        );
        if note.is_null() {
            say(out, "home=unavailable\n");
        } else {
            say(note, "from home");
            abi_application::xila_file_close(note);
            let note = open_home(startup, "note.txt", abi_application::XILA_OPEN_READ);
            let mut buffer = [0u8; 32];
            let mut count = 0;
            abi_application::xila_file_read(
                note,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut count,
            );
            abi_application::xila_file_close(note);
            say(
                out,
                &format!("home={}\n", String::from_utf8_lossy(&buffer[..count])),
            );
        }

        match arguments.get(1).copied() {
            Some("fail") => 3,
            Some("minimum") => i32::MIN,
            Some("exit") => {
                say(out, "before exit\n");
                abi_application::xila_process_exit(startup.process, 9);
                say(out, "after exit\n");
                1
            }
            _ => 0,
        }
    }
}

linked_executable!(unsafe CReport => c_report, stack_size = STACK, home = LinkedHome::read_write(PathOwned::new("/".into()).unwrap()));

// ---------------------------------------------------------------------------------------
// Entries written with `xila-sys`, in linked mode.
// ---------------------------------------------------------------------------------------

/// What each concurrent run did, in order: `(who, step)`.
static EVENTS: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

fn sys_say(startup: &xila_sys::XilaStartup, file: *mut xila_sys::File, text: &str) {
    let api = unsafe { &*startup.api };
    let mut count = 0;
    unsafe { (api.file_write)(file, text.as_ptr().cast(), text.len(), &mut count) };
}

fn sys_environment(startup: &xila_sys::XilaStartup, name: &str) -> String {
    let api = unsafe { &*startup.api };
    let mut buffer = [0 as core::ffi::c_char; 64];
    let mut length = 0;
    let status = unsafe {
        (api.env_get)(
            startup.process,
            name.as_ptr().cast(),
            name.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut length,
        )
    };
    assert_eq!(status, xila_sys::Status::Ok);
    String::from_utf8_lossy(unsafe {
        core::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length)
    })
    .into_owned()
}

/// Sleeps in steps. After each step it asks the runtime for its startup data again: a
/// program that shared a global with the other run would read the other's.
unsafe fn sys_main(initial: *const xila_sys::XilaStartup) -> i32 {
    let me = xila_sys::runtime::startup().expect("startup");
    let api = xila_sys::runtime::api().expect("api");
    let mut consistent = ptr::eq(me, initial) && ptr::eq(api, me.api);
    let who = unsafe { &*me.arguments };
    let who = String::from_utf8_lossy(unsafe {
        core::slice::from_raw_parts(who.data.cast(), who.length)
    })
    .into_owned();
    let zone = sys_environment(me, "WHO");
    for step in 0..4 {
        EVENTS.lock().unwrap().push((who.clone(), step));
        unsafe { (api.time_sleep)(5_000_000) };
        let again = xila_sys::runtime::startup().expect("startup");
        let first = unsafe { &*again.arguments };
        let again_name = String::from_utf8_lossy(unsafe {
            core::slice::from_raw_parts(first.data.cast(), first.length)
        });
        consistent &=
            ptr::eq(again, me) && again_name == who && sys_environment(again, "WHO") == zone;
    }
    sys_say(
        me,
        me.standard_output,
        &format!("{who} {zone} consistent={consistent}\n"),
    );
    i32::from(!consistent)
}

mod first {
    xila_sys::entry!(super::sys_main);
}
mod second {
    xila_sys::entry!(super::sys_main, symbol = xila_second_entry);
}

linked_executable!(unsafe RustFirst => first::xila_entry, stack_size = STACK);
linked_executable!(unsafe RustSecond => second::xila_second_entry, stack_size = STACK);

// ---------------------------------------------------------------------------------------
// Localized C fixtures: two copies of one C program, each with every symbol but its entry
// localized by the build script, linked into this binary (see build.rs and
// tests/linked_c/program.c). Both copies define `helper`, `malloc` and `static int state`.
// ---------------------------------------------------------------------------------------

mod linked_c {
    include!(concat!(env!("OUT_DIR"), "/linked_c_fixtures.rs"));
}

linked_executable!(unsafe CAlpha => linked_c::xila_c_alpha_entry, stack_size = STACK);
linked_executable!(unsafe CBeta => linked_c::xila_c_beta_entry, stack_size = STACK);

// ---------------------------------------------------------------------------------------
// Exclusive programs: one instance at a time.
// ---------------------------------------------------------------------------------------

/// Returns at once.
unsafe extern "C" fn quick(_startup: *const XilaStartup) -> i32 {
    0
}

/// Writes one ready byte to the output stream, then suspends reading one byte from the
/// input stream: the test reads the ready byte, checks a concurrent launch fails busy, and
/// releases the run by writing the byte. No sleeps anywhere.
unsafe extern "C" fn slow(startup: *const XilaStartup) -> i32 {
    let startup = unsafe { &*startup };
    unsafe {
        let mut count = 0;
        abi_application::xila_file_write(
            startup.standard_output,
            c"R".as_ptr().cast::<c_void>(),
            1,
            &mut count,
        );
        let mut byte = [0u8; 1];
        let mut read = 0;
        abi_application::xila_file_read(
            startup.standard_input,
            byte.as_mut_ptr().cast::<c_void>(),
            1,
            &mut read,
        );
    }
    0
}

linked_executable!(unsafe ExclusiveSlow => slow, exclusive, stack_size = STACK);

/// The single-instance slot of the direct `run_linked_exclusive` test below.
static EXCLUSIVE_SLOT: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------------------
// Fixture.
// ---------------------------------------------------------------------------------------

struct Sink;
impl DirectBaseOperations for Sink {
    fn read(
        &self,
        _: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn write(
        &self,
        _: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Ok(buffer.len()))
    }
}
impl MountOperations for Sink {
    fn unmount(&self) -> file_system::Result<()> {
        Ok(())
    }
}
impl DirectCharacterDevice for Sink {}

async fn setup() -> (&'static ExecutableContext, TaskIdentifier) {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let tasks = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    let parent = tasks.get_current_task_identifier().await;
    let device = file_system::MemoryDevice::<512>::new_static(256 * 1024);
    let file_system =
        little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, time, file_system).unwrap(),
    ));
    let context = Box::leak(Box::new(ExecutableContext::new(tasks, users, vfs, time)));
    vfs.mount_character_device(parent, "/sink", Sink)
        .await
        .unwrap();
    mount_executables!(
        context,
        vfs,
        parent,
        &[
            (&"/report", CReport),
            (&"/first", RustFirst),
            (&"/second", RustSecond),
            (&"/c-alpha", CAlpha),
            (&"/c-beta", CBeta),
            (&"/exclusive-slow", ExclusiveSlow)
        ]
    )
    .await
    .unwrap();
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    File::open(vfs, parent, "/input", create)
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();
    (context, parent)
}

async fn streams(
    context: &'static ExecutableContext,
    parent: TaskIdentifier,
    label: &str,
) -> Standard<'static> {
    let vfs = context.virtual_file_system;
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let input = File::open(vfs, parent, "/input", create).await.unwrap();
    let output = File::open(vfs, parent, format!("/{label}.out").as_str(), create)
        .await
        .unwrap();
    let error = File::open(vfs, parent, format!("/{label}.err").as_str(), create)
        .await
        .unwrap();
    Standard::new(context, input, output, error)
}

async fn read_all(vfs: &'static VirtualFileSystem, parent: TaskIdentifier, path: &str) -> String {
    let mut file = File::open(vfs, parent, path, AccessFlags::Read.into())
        .await
        .unwrap();
    let mut text = Vec::new();
    let mut chunk = [0; 256];
    loop {
        let count = file.read(&mut chunk).await.unwrap();
        if count == 0 {
            break;
        }
        text.extend_from_slice(&chunk[..count]);
    }
    file.close(vfs).await.unwrap();
    String::from_utf8(text).unwrap()
}

// ---------------------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------------------

/// Return success only when startup has no home capability.
unsafe extern "C" fn check_no_home(startup: *const XilaStartup) -> i32 {
    // SAFETY: The host passes a live startup block to the entry.
    i32::from(!unsafe { (*startup).home }.is_null())
}

#[task::test]
async fn linked_program_starts_without_a_missing_home() {
    let (context, parent) = setup().await;
    let standard = streams(context, parent, "missing-home").await;
    // SAFETY: The entry reads only the startup block and returns without unwinding.
    let result = unsafe {
        run_linked(
            check_no_home,
            LinkedOptions::new().stack_size(STACK),
            standard,
            vec![],
        )
    }
    .await;
    assert_eq!(result, Ok(()));
}

async fn assert_linked_start_failure(standard: Standard<'static>) {
    // SAFETY: The entry returns immediately without unwinding.
    let result = unsafe {
        run_linked(
            quick,
            LinkedOptions::new().stack_size(STACK),
            standard,
            vec![],
        )
    }
    .await;
    assert_eq!(result, Err(START_FAILURE));
}

#[task::test]
async fn linked_program_rejects_a_file_as_its_home() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    vfs.create_directory(parent, &"/users").await.unwrap();
    let create = Flags::new(AccessFlags::Read, Some(CreateFlags::Create), None);
    File::open(vfs, parent, users::ROOT_HOME, create)
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();

    assert_linked_start_failure(streams(context, parent, "file-home").await).await;
}

#[task::test]
async fn linked_program_rejects_a_home_it_cannot_access() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    let standard = streams(context, parent, "denied-home").await;
    let user = users::UserIdentifier::new(1000);
    let group = users::GroupIdentifier::new(1000);
    let path = "/users/reader";
    context
        .users_manager
        .add_group(group, "staff", &[])
        .await
        .unwrap();
    context
        .users_manager
        .add_user(user, "reader", group, path)
        .await
        .unwrap();
    vfs.create_directory(parent, &"/users").await.unwrap();
    for ancestor in ["/", "/users"] {
        vfs.set_permissions(parent, ancestor, Permissions::from_octal(0o755).unwrap())
            .await
            .unwrap();
    }
    vfs.create_directory(parent, &path).await.unwrap();
    vfs.set_permissions(parent, path, Permissions::USER_FULL)
        .await
        .unwrap();
    context.task_manager.set_user(parent, user).await.unwrap();

    assert_linked_start_failure(standard).await;
}

#[task::test]
async fn linked_program_rejects_an_invalid_recorded_home() {
    let (context, parent) = setup().await;
    let standard = streams(context, parent, "invalid-home").await;
    let user = users::UserIdentifier::new(1000);
    context
        .users_manager
        .add_user(user, "reader", users::GroupIdentifier::ROOT, "/invalid")
        .await
        .unwrap();
    context.task_manager.set_user(parent, user).await.unwrap();

    assert_linked_start_failure(standard).await;
}

#[task::test]
async fn c_abi_entry_runs_as_a_full_process() {
    let (context, parent) = setup().await;
    let (tasks, vfs) = (context.task_manager, context.virtual_file_system);
    tasks
        .set_environment_variable(parent, "FROM_HOST", "host")
        .await
        .unwrap();
    tasks
        .set_environment_variable(parent, "TZ", "Europe/Paris")
        .await
        .unwrap();
    let before = tasks.get_task_count().await;

    for (label, mode, status) in [
        ("zero", "ok", 0),
        ("three", "fail", -3),
        // A status of `i32::MIN` must not overflow the conversion to an exit code.
        ("minimum", "minimum", -(i32::MIN.unsigned_abs() as isize)),
        ("exit", "exit", -9),
    ] {
        let standard = streams(context, parent, label).await;
        let handle = execute(
            context,
            "/report",
            vec!["report".into(), mode.into()],
            standard,
            None,
        )
        .await
        .unwrap();
        assert_eq!(handle.join().await, status, "{label}");
        let out = read_all(vfs, parent, &format!("/{label}.out")).await;
        assert!(out.contains("name=report\n"), "{out}");
        assert!(out.contains(&format!("arguments=report,{mode}\n")), "{out}");
        assert!(out.contains("same_startup=true\n"), "{out}");
        assert!(out.contains("host=host\n"), "{out}");
        assert!(out.contains("zone=Europe/Paris\n"), "{out}");
        assert!(out.contains("home=from home\n"), "{out}");
        assert_eq!(
            read_all(vfs, parent, &format!("/{label}.err")).await,
            "to stderr\n"
        );
        // The exit never reached the code after `xila_process_exit`.
        assert_eq!(out.contains("before exit"), mode == "exit");
        assert!(!out.contains("after exit"));
    }
    assert_eq!(tasks.get_task_count().await, before);
    // The program wrote its note in the home directory it was granted.
    assert_eq!(read_all(vfs, parent, "/note.txt").await, "from home");
}

#[task::test]
async fn rust_entries_run_concurrently_without_sharing_startup_data() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    EVENTS.lock().unwrap().clear();
    let mut handles = Vec::new();
    for (path, who) in [("/first", "alpha"), ("/second", "beta")] {
        let standard = streams(context, parent, who).await;
        let launch = Launch::default().with_environment(vec![EnvironmentVariable::new("WHO", who)]);
        handles.push(
            execute_with(context, path, vec![who.into()], standard, None, launch)
                .await
                .unwrap(),
        );
    }
    for handle in handles {
        assert_eq!(handle.join().await, 0);
    }
    assert_eq!(
        read_all(vfs, parent, "/alpha.out").await,
        "alpha alpha consistent=true\n"
    );
    assert_eq!(
        read_all(vfs, parent, "/beta.out").await,
        "beta beta consistent=true\n"
    );

    // Both programs were suspended mid-run at the same time: their steps interleave.
    let events = EVENTS.lock().unwrap().clone();
    assert_eq!(events.len(), 8);
    let last_alpha = events.iter().rposition(|(who, _)| who == "alpha").unwrap();
    let first_beta = events.iter().position(|(who, _)| who == "beta").unwrap();
    assert!(
        first_beta < last_alpha,
        "the runs did not overlap: {events:?}"
    );
}

/// Sizes of the two declarations of the startup block agree, which `linked_entry` assumes.
#[test]
fn sdk_and_host_declare_the_same_startup_block() {
    assert_eq!(size_of::<xila_sys::XilaStartup>(), size_of::<XilaStartup>());
    assert_eq!(
        align_of::<xila_sys::XilaStartup>(),
        align_of::<XilaStartup>()
    );
}

// The tests run in parallel in one process, so each gets its own flags: sharing them made
// the assertions below race the other test's runs.
static CANCEL_BLOCKED: AtomicBool = AtomicBool::new(false);
static CANCEL_OPENED: AtomicUsize = AtomicUsize::new(0);
static EXCLUSIVE_BLOCKED: AtomicBool = AtomicBool::new(false);
static EXCLUSIVE_OPENED: AtomicUsize = AtomicUsize::new(0);

/// Opens a file and a pipe, then waits on the pipe, which no one writes to.
unsafe fn blocked_run(
    blocked: &AtomicBool,
    opened: &AtomicUsize,
    startup: *const XilaStartup,
) -> i32 {
    let startup = unsafe { &*startup };
    unsafe {
        let mut file = ptr::null_mut();
        let status = abi_application::xila_file_open(
            startup.home,
            c"probe".as_ptr(),
            5,
            abi_application::XILA_OPEN_READ | abi_application::XILA_OPEN_WRITE,
            &mut file,
        );
        assert_eq!(status, abi_application::Status::Ok);
        opened.fetch_add(1, Ordering::SeqCst);
        let (mut reader, mut writer) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            abi_application::xila_pipe_create(startup.process, 0, &mut reader, &mut writer),
            abi_application::Status::Ok
        );
        blocked.store(true, Ordering::SeqCst);
        let mut buffer = [0u8; 4];
        let mut count = 0;
        abi_application::xila_file_read(
            reader,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut count,
        );
        // Never reached: the program is cancelled while it waits.
        blocked.store(false, Ordering::SeqCst);
    }
    0
}

unsafe extern "C" fn blocked(startup: *const XilaStartup) -> i32 {
    // SAFETY: a test entry that obeys the startup rules.
    unsafe { blocked_run(&CANCEL_BLOCKED, &CANCEL_OPENED, startup) }
}

unsafe extern "C" fn blocked_exclusive(startup: *const XilaStartup) -> i32 {
    // SAFETY: a test entry that obeys the startup rules.
    unsafe { blocked_run(&EXCLUSIVE_BLOCKED, &EXCLUSIVE_OPENED, startup) }
}

#[task::test]
async fn cancelled_program_releases_its_stack_state_and_handles() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    // A device that stays busy while any handle to it is open.
    vfs.mount_character_device(parent, "/probe", Sink)
        .await
        .unwrap();
    vfs.set_permissions(parent, "/probe", Permissions::ALL_READ_WRITE)
        .await
        .unwrap();
    let home = || LinkedHome::read_write(PathOwned::new("/".into()).unwrap());
    let start = |standard| {
        // SAFETY: `blocked` is a test entry that obeys the startup rules, fits on the
        // stack and returns without unwinding.
        unsafe {
            run_linked(
                blocked,
                LinkedOptions::new().stack_size(STACK).home(home()),
                standard,
                vec![],
            )
        }
    };

    // A first, complete run warms every lazily initialized service, so the accounting below
    // counts only what the program itself holds.
    let standard = streams(context, parent, "warm").await;
    let mut warm = start(standard);
    poll_fn(|cx| {
        assert!(warm.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(warm);

    CANCEL_BLOCKED.store(false, Ordering::SeqCst);
    // Everything the run allocates, its streams included, is counted.
    let before = MemoryManager::thread_allocations();
    let standard = streams(context, parent, "cancel").await;
    let mut program = start(standard);
    poll_fn(|cx| {
        assert!(program.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(
        CANCEL_BLOCKED.load(Ordering::SeqCst),
        "the program did not reach its blocking call"
    );
    drop(program);
    let held = MemoryManager::thread_allocations().since(before);
    // The coroutine is gone: its read never completed.
    assert!(CANCEL_BLOCKED.load(Ordering::SeqCst));
    // The coroutine stack, the process state and the heap went back to the allocator.
    assert!(held.deallocated_bytes() >= STACK, "{held:?}");
    assert_eq!(
        held.net_bytes(),
        0,
        "a cancelled program leaked memory: {held:?}"
    );
    // Every handle of the program is closed, so the device can go away.
    vfs.unmount("/probe", false).await.unwrap();
    assert_eq!(CANCEL_OPENED.load(Ordering::SeqCst), 2);
}

// ---------------------------------------------------------------------------------------
// Exclusive linked programs: one instance at a time, released on return and on
// cancellation.
// ---------------------------------------------------------------------------------------

/// One instance of the program is suspended reading from a pipe. A second concurrent
/// launch fails immediately with `BUSY_FAILURE` and never touches the entry; dropping
/// the first (a cancellation mid-pipe-read) releases the slot, and so does a normal
/// return.
#[task::test]
async fn exclusive_run_fails_busy_while_suspended_and_recovers_after_cancellation() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    vfs.mount_character_device(parent, "/probe", Sink)
        .await
        .unwrap();
    vfs.set_permissions(parent, "/probe", Permissions::ALL_READ_WRITE)
        .await
        .unwrap();
    let home = || LinkedHome::read_write(PathOwned::new("/".into()).unwrap());
    // SAFETY: `blocked` and `quick` are test entries that obey the startup rules, fit on
    // the stack and return without unwinding.
    let start_blocked = |standard| unsafe {
        run_linked_exclusive(
            &EXCLUSIVE_SLOT,
            blocked_exclusive,
            LinkedOptions::new().stack_size(STACK).home(home()),
            standard,
            vec![],
        )
    };
    let start_quick = |standard| unsafe {
        run_linked_exclusive(
            &EXCLUSIVE_SLOT,
            quick,
            LinkedOptions::new().stack_size(STACK),
            standard,
            vec![],
        )
    };

    EXCLUSIVE_BLOCKED.store(false, Ordering::SeqCst);

    // The first instance suspends on the pipe read.
    let mut first = start_blocked(streams(context, parent, "excl-first").await);
    poll_fn(|cx| {
        assert!(first.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(EXCLUSIVE_BLOCKED.load(Ordering::SeqCst));
    let opened_before = EXCLUSIVE_OPENED.load(Ordering::SeqCst);

    // A second launch while the first is suspended fails at once, without running the
    // entry: the open counter does not move.
    let second = start_blocked(streams(context, parent, "excl-second").await);
    assert_eq!(second.await, Err(BUSY_FAILURE));
    assert_eq!(
        EXCLUSIVE_OPENED.load(Ordering::SeqCst),
        opened_before,
        "a busy launch touched the entry or its globals"
    );

    // Cancelling the first (dropping the future mid-pipe-read) releases the slot: a new
    // launch runs to completion.
    drop(first);
    assert_eq!(
        start_quick(streams(context, parent, "excl-third").await).await,
        Ok(())
    );

    // A blocked instance can start again; dropping it releases the slot once more, and a
    // normal return releases it too.
    let mut fourth = start_blocked(streams(context, parent, "excl-fourth").await);
    poll_fn(|cx| {
        assert!(fourth.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(EXCLUSIVE_BLOCKED.load(Ordering::SeqCst));
    drop(fourth);
    assert_eq!(
        start_quick(streams(context, parent, "excl-fifth").await).await,
        Ok(())
    );
}

/// The `exclusive` form of `linked_executable!`, mounted and run through `execute`: while
/// one instance is suspended, a concurrent launch's task exits with `BUSY_FAILURE`; once
/// the first finishes normally, a new launch succeeds. The first instance's streams are
/// pipes held by the test, so the handshake is deterministic: the entry writes one ready
/// byte to its stdout pipe and then blocks reading its stdin pipe; the test reads the
/// ready byte, launches the second instance, then releases the first by writing its stdin.
#[task::test]
async fn exclusive_executable_fails_a_concurrent_launch_with_busy_then_relaunches() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let (mut out_reader, out_writer) = vfs
        .create_unnamed_pipe(512, StateFlags::empty())
        .await
        .unwrap();
    let (in_reader, mut in_writer) = vfs
        .create_unnamed_pipe(512, StateFlags::empty())
        .await
        .unwrap();
    let error = File::open(vfs, parent, "/slow-first.err", create)
        .await
        .unwrap();

    let first = execute(
        context,
        "/exclusive-slow",
        vec![],
        Standard::new(context, in_reader, out_writer, error),
        None,
    )
    .await
    .unwrap();
    // Read the entry's ready byte: this suspends until the entry writes it, so once it
    // arrives the first instance holds the slot and is suspended on its stdin read.
    let mut ready = [0u8; 1];
    assert_eq!(out_reader.read(&mut ready).await.unwrap(), 1);
    assert_eq!(&ready, b"R");

    // A concurrent launch while the first is suspended fails with BUSY_FAILURE.
    let second = execute(
        context,
        "/exclusive-slow",
        vec![],
        streams(context, parent, "slow-second").await,
        None,
    )
    .await
    .unwrap();
    assert_eq!(second.join().await, -(BUSY_FAILURE.get() as isize));

    // Release the first instance: its stdin read completes and it returns normally, which
    // releases the slot.
    in_writer.write(b"R").await.unwrap();
    assert_eq!(first.join().await, 0);

    // A new launch after the normal return succeeds (its stdin is an empty file, so the
    // entry's read returns EOF at once).
    let third = execute(
        context,
        "/exclusive-slow",
        vec![],
        streams(context, parent, "slow-third").await,
        None,
    )
    .await
    .unwrap();
    assert_eq!(third.join().await, 0);
}

/// Two localized C programs, both defining `helper`, `malloc` and `static int state`, run
/// concurrently through `XilaStartup` with full isolation: each sees its own `state`
/// count 1, 2, 3 and returns zero. Skipped when the build host had no clang/ld.lld/
/// llvm-objcopy to build the fixtures.
#[task::test]
async fn two_localized_c_programs_run_concurrently_with_isolated_globals() {
    if !linked_c::AVAILABLE {
        return;
    }
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    let mut handles = Vec::new();
    for (path, name, label) in [
        ("/c-alpha", "alpha", "calpha"),
        ("/c-beta", "beta", "cbeta"),
    ] {
        let standard = streams(context, parent, label).await;
        handles.push(
            execute(context, path, vec![name.into()], standard, None)
                .await
                .unwrap(),
        );
    }
    for handle in handles {
        assert_eq!(handle.join().await, 0);
    }
    for (name, label) in [("alpha", "calpha"), ("beta", "cbeta")] {
        let out = read_all(vfs, parent, &format!("/{label}.out")).await;
        assert_eq!(out.matches("state=").count(), 3, "{label}: {out}");
        for step in 1..=3 {
            assert!(
                out.contains(&format!("{name} state={step}\n")),
                "{label}: {out}"
            );
        }
    }
}

/// The same localized C program run twice sequentially: the SDK resets its own state on
/// every launch, and the fixture resets its application-owned `state` at the top of its
/// entry, so each run counts 1, 2, 3 again and returns zero. Skipped when the build host
/// had no clang/ld.lld/llvm-objcopy to build the fixtures.
#[task::test]
async fn a_localized_c_program_runs_sequentially_with_fresh_state_each_time() {
    if !linked_c::AVAILABLE {
        return;
    }
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    for round in 0..2 {
        let standard = streams(context, parent, &format!("cseq{round}")).await;
        let handle = execute(context, "/c-alpha", vec!["alpha".into()], standard, None)
            .await
            .unwrap();
        assert_eq!(handle.join().await, 0, "round {round}");
        let out = read_all(vfs, parent, &format!("/cseq{round}.out")).await;
        assert_eq!(out.matches("state=").count(), 3, "round {round}: {out}");
        for step in 1..=3 {
            assert!(
                out.contains(&format!("alpha state={step}\n")),
                "round {round}: {out}"
            );
        }
    }
}
