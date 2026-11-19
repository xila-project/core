#![cfg(target_os = "linux")]
//! Booting loads only real account records and logs only genuine failures.
use authentication::Context;
use file_system::Kind;
use log::LoggerTrait;
use std::fmt;
use std::sync::Mutex;
use users::GroupIdentifier;
use virtual_file_system::{Directory, File, ItemStatic};
testing_support::instantiate_global_allocator!();

/// Collects every message written through the global logger.
struct Capture(Mutex<Vec<String>>);

impl LoggerTrait for Capture {
    fn enabled(&self, _: log::Level) -> bool {
        true
    }

    fn write(&self, arguments: fmt::Arguments) {
        self.0.lock().unwrap().push(arguments.to_string());
    }
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn messages() -> Vec<String> {
    CAPTURE.0.lock().unwrap().clone()
}

async fn fixture() -> Context<'static> {
    let task_manager = task::test_manager();
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let fs = little_fs::FileSystem::new_format(
        file_system::MemoryDevice::<512>::new_static(512 * 1024),
        little_fs::Options::new(256),
    )
    .unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time, fs).unwrap(),
    ));
    let task = task_manager.get_current_task_identifier().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();
    for (path, device) in [
        (
            "/devices/random/source",
            ItemStatic::CharacterDevice(&testing_support::RandomDevice),
        ),
        (
            "/devices/compute/hasher",
            ItemStatic::CharacterDevice(&testing_support::HashDevice),
        ),
    ] {
        vfs.mount_static(task, &path, device).await.unwrap();
    }
    Context {
        virtual_file_system: vfs,
        task_manager,
        users_manager,
        task,
    }
}

async fn entries(context: &Context<'_>, path: &str) -> Vec<(String, Kind)> {
    Directory::open(context.virtual_file_system, context.task, path)
        .await
        .unwrap()
        .map(|entry| (entry.name, entry.kind))
        .collect()
}

/// Load every account into a fresh registry, as a boot does.
async fn load(context: &Context<'_>) -> &'static users::Manager {
    let users_manager: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    authentication::load_all_users_and_groups(&Context {
        virtual_file_system: context.virtual_file_system,
        task_manager: context.task_manager,
        users_manager,
        task: context.task,
    })
    .await
    .unwrap();
    users_manager
}

#[task::test]
async fn loading_skips_special_entries_and_logs_only_real_failures() {
    log::initialize(&CAPTURE).unwrap();
    let context = fixture().await;
    authentication::create_group(&context, "people", Some(GroupIdentifier::new(1000)))
        .await
        .unwrap();
    let alice = authentication::create_user(
        &context,
        "alice",
        "correct horse battery staple",
        GroupIdentifier::new(1000),
        None,
    )
    .await
    .unwrap();

    // The directory iterator reports the directory itself and its parent.
    let users = entries(&context, "/system/accounts/users").await;
    assert!(users.contains(&(".".into(), Kind::Directory)), "{users:?}");
    assert!(users.contains(&("..".into(), Kind::Directory)), "{users:?}");

    // A clean disk loads without a single skipped record.
    let users_manager = load(&context).await;
    assert_eq!(users_manager.get_user_name(alice).await.unwrap(), "alice");
    assert_eq!(
        users_manager
            .get_group_name(GroupIdentifier::new(1000))
            .await
            .unwrap(),
        "people"
    );
    let skipped: Vec<_> = messages()
        .into_iter()
        .filter(|message| message.contains("Skipped an account record"))
        .collect();
    assert!(skipped.is_empty(), "{skipped:?}");

    // A stray directory is not a record and is ignored silently.
    virtual_file_system::Directory::create(
        context.virtual_file_system,
        context.task,
        "/system/accounts/users/stray",
    )
    .await
    .unwrap();
    load(&context).await;
    assert!(messages().is_empty(), "{:?}", messages());

    // Records that genuinely fail to load are still reported, once each.
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        &"/system/accounts/users/broken",
        b"not json",
    )
    .await
    .unwrap();
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        &"/system/accounts/groups/broken",
        b"not json",
    )
    .await
    .unwrap();
    let users_manager = load(&context).await;
    assert_eq!(users_manager.get_user_name(alice).await.unwrap(), "alice");
    let skipped = messages();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(
        skipped
            .iter()
            .any(|message| message.contains("Skipped a group record")),
        "{skipped:?}"
    );
    assert!(
        skipped
            .iter()
            .any(|message| message.contains("Skipped an account record")),
        "{skipped:?}"
    );
}
