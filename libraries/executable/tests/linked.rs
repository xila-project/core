//! Statically linked native programs (`native-linked`): C-ABI and Rust (`xila-sys`) entries
//! mounted as executables, run concurrently on one executor, and cancelled while suspended.
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]

use core::{ffi::c_void, ptr, task::Poll};
use portable_atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{future::poll_fn, sync::Mutex};

use abi_application::{File as NativeFile, XilaStartup};
use executable::{
    ExecutableContext, Launch, LinkedHome, LinkedOptions, Standard, execute, execute_with,
    linked_executable, mount_executables, run_linked,
};
use file_system::{
    AccessFlags, CreateFlags, DirectBaseOperations, DirectCharacterDevice, Flags, MountOperations,
    PathOwned, Permissions, Size,
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
            (&"/second", RustSecond)
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

static BLOCKED: AtomicBool = AtomicBool::new(false);
static OPENED: AtomicUsize = AtomicUsize::new(0);

/// Opens a file and a pipe, then waits on the pipe, which no one writes to.
unsafe extern "C" fn blocked(startup: *const XilaStartup) -> i32 {
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
        OPENED.fetch_add(1, Ordering::SeqCst);
        let (mut reader, mut writer) = (ptr::null_mut(), ptr::null_mut());
        assert_eq!(
            abi_application::xila_pipe_create(startup.process, 0, &mut reader, &mut writer),
            abi_application::Status::Ok
        );
        BLOCKED.store(true, Ordering::SeqCst);
        let mut buffer = [0u8; 4];
        let mut count = 0;
        abi_application::xila_file_read(
            reader,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut count,
        );
        // Never reached: the program is cancelled while it waits.
        BLOCKED.store(false, Ordering::SeqCst);
    }
    0
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

    BLOCKED.store(false, Ordering::SeqCst);
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
        BLOCKED.load(Ordering::SeqCst),
        "the program did not reach its blocking call"
    );
    drop(program);
    let held = MemoryManager::thread_allocations().since(before);
    // The coroutine is gone: its read never completed.
    assert!(BLOCKED.load(Ordering::SeqCst));
    // The coroutine stack, the process state and the heap went back to the allocator.
    assert!(held.deallocated_bytes() >= STACK, "{held:?}");
    assert_eq!(
        held.net_bytes(),
        0,
        "a cancelled program leaked memory: {held:?}"
    );
    // Every handle of the program is closed, so the device can go away.
    vfs.unmount("/probe", false).await.unwrap();
    assert_eq!(OPENED.load(Ordering::SeqCst), 2);
}
