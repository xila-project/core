//! Launching a program with an explicit identity (`Launch::with_identity`).
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::sync::Mutex;

use executable::{
    Error, ExecutableContext, ExecutableFormat, ExecutableTrait, FormatHandler, FormatLaunch,
    FormatRequest, Launch, MainFuture, Standard, execute_with, mount_executables,
};
use file_system::{AccessFlags, CreateFlags, Flags, Permissions};
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, VirtualFileSystem};

testing_support::instantiate_global_allocator!();

const USER: UserIdentifier = UserIdentifier::new(1000);
const GROUP: GroupIdentifier = GroupIdentifier::new(1000);

/// The identity each run of [`Identify`] saw, in order.
static SEEN: Mutex<Vec<(UserIdentifier, GroupIdentifier)>> = Mutex::new(Vec::new());

/// Whether each run of [`Identify`] and a task it spawned were marked as
/// running with a granted identity, in order.
static GRANTED: Mutex<Vec<(bool, bool)>> = Mutex::new(Vec::new());

struct Identify;
impl ExecutableTrait for Identify {
    fn main(standard: Standard<'static>, _: Vec<String>) -> MainFuture {
        Box::pin(async move {
            let manager = standard.context.task_manager;
            let task = manager.get_current_task_identifier().await;
            let user = manager.get_user(task).await.unwrap();
            let group = manager.get_group(task).await.unwrap();
            SEEN.lock().unwrap().push((user, group));
            let granted = manager.is_identity_granted(task).await.unwrap();
            let (child, _) = manager
                .spawn(task, "identify child", None, async move |child| {
                    manager.is_identity_granted(child).await.unwrap()
                })
                .await
                .unwrap();
            let child_granted = child.join().await;
            GRANTED.lock().unwrap().push((granted, child_granted));
            Ok(())
        })
    }
}

/// A format handler that must never be reached by a launch with an identity.
struct Refused;
impl FormatHandler for Refused {
    fn launch(&self, _: FormatRequest) -> FormatLaunch<'_> {
        panic!("a format handler was asked to launch with an identity");
    }
}
static FORMATS: [ExecutableFormat; 1] = [ExecutableFormat::new(*b"\0asm", &Refused)];

/// The context, its file system, the calling task, and a null device opened
/// while the task is still root, duplicated for the standard streams of
/// every launch.
async fn environment() -> (
    &'static ExecutableContext,
    &'static VirtualFileSystem,
    TaskIdentifier,
    File<'static>,
) {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let tasks = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    users.add_group(GROUP, "staff", &[]).await.unwrap();
    users
        .add_user(USER, "user", GROUP, "/users/user")
        .await
        .unwrap();
    let task = tasks.get_current_task_identifier().await;
    let device = file_system::MemoryDevice::<512>::new_static(64 * 1024);
    let file_system =
        little_fs::FileSystem::new_format(device, little_fs::Options::new(64)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, time, file_system).unwrap(),
    ));
    // Other users may traverse the root directory.
    vfs.set_permissions(task, "/", Permissions::from_octal(0o755).unwrap())
        .await
        .unwrap();
    vfs.mount_character_device(task, "/null", drivers_core::NullDevice)
        .await
        .unwrap();
    let mut context = ExecutableContext::new(tasks, users, vfs, time);
    context.formats = &FORMATS;
    let context: &'static ExecutableContext = Box::leak(Box::new(context));

    mount_executables!(context, vfs, task, &[(&"/identify", Identify)])
        .await
        .unwrap();
    let create = Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None);
    let mut guest = File::open(vfs, task, "/guest", create).await.unwrap();
    guest.write(b"\0asm\x01\0\0\0").await.unwrap();
    guest.close(vfs).await.unwrap();
    vfs.set_permissions(task, "/guest", Permissions::EXECUTABLE)
        .await
        .unwrap();
    let null = File::open(vfs, task, "/null", AccessFlags::READ_WRITE.into())
        .await
        .unwrap();
    (context, vfs, task, null)
}

async fn run(
    context: &'static ExecutableContext,
    null: &File<'static>,
    path: &str,
    launch: Launch,
) -> Result<isize, Error> {
    let standard = Standard::new(
        context,
        null.duplicate().await.unwrap(),
        null.duplicate().await.unwrap(),
        null.duplicate().await.unwrap(),
    );
    let handle = execute_with(context, path, vec![], standard, None, launch).await?;
    Ok(handle.join().await)
}

#[task::test]
async fn launch_identity_applies_to_the_child_only_and_is_checked_for_execution() {
    let (context, vfs, task, null) = environment().await;
    let manager = context.task_manager;
    manager.set_user(task, USER).await.unwrap();
    manager.set_group(task, GROUP).await.unwrap();

    // Without an identity, the child inherits the caller's.
    assert_eq!(
        run(context, &null, "/identify", Launch::default())
            .await
            .unwrap(),
        0
    );
    // With one, the child runs as it and the caller keeps its own.
    let root = Launch::default().with_identity(UserIdentifier::ROOT, GroupIdentifier::ROOT);
    assert_eq!(run(context, &null, "/identify", root).await.unwrap(), 0);
    assert_eq!(manager.get_user(task).await.unwrap(), USER);
    assert_eq!(manager.get_group(task).await.unwrap(), GROUP);
    assert_eq!(
        *SEEN.lock().unwrap(),
        [(USER, GROUP), (UserIdentifier::ROOT, GroupIdentifier::ROOT)]
    );
    // Only a launch with an identity marks the program, and every task it
    // starts, as running with a granted identity; the caller is never marked.
    assert_eq!(*GRANTED.lock().unwrap(), [(false, false), (true, true)]);
    assert!(!manager.is_identity_granted(task).await.unwrap());

    // The execute permission is checked for the identity the program runs as:
    // owner (root) may read and execute, others may only read.
    manager.set_user(task, UserIdentifier::ROOT).await.unwrap();
    vfs.set_permissions(task, "/identify", Permissions::from_octal(0o504).unwrap())
        .await
        .unwrap();
    manager.set_user(task, USER).await.unwrap();
    assert!(matches!(
        run(context, &null, "/identify", Launch::default()).await,
        Err(Error::PermissionDenied)
    ));
    let root = Launch::default().with_identity(UserIdentifier::ROOT, GroupIdentifier::ROOT);
    assert_eq!(run(context, &null, "/identify", root).await.unwrap(), 0);
    assert_eq!(manager.get_user(task).await.unwrap(), USER);
}

#[task::test]
async fn launch_identity_is_refused_for_file_formats() {
    let (context, _, task, null) = environment().await;
    context.task_manager.set_user(task, USER).await.unwrap();
    let root = Launch::default().with_identity(UserIdentifier::ROOT, GroupIdentifier::ROOT);
    assert!(matches!(
        run(context, &null, "/guest", root).await,
        Err(Error::PermissionDenied)
    ));
}
