#![cfg(target_os = "linux")]
use abi_definitions as _;
use authentication::{Context, initialize_seeded};
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, ItemStatic};
drivers_std::instantiate_global_allocator!();

async fn fixture() -> Context<'static> {
    let task_manager = task::test_manager();
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));
    let fs = little_fs::FileSystem::new_format(
        file_system::MemoryDevice::<512>::new_static(512 * 1024),
        256,
    )
    .unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time, fs).unwrap(),
    ));
    let task = task_manager.get_current_task_identifier().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();
    vfs.mount_static(
        task,
        &"/devices/random/source",
        ItemStatic::CharacterDevice(&drivers_shared::devices::RandomDevice),
    )
    .await
    .unwrap();
    vfs.mount_static(
        task,
        &"/devices/compute/hasher",
        ItemStatic::CharacterDevice(&drivers_shared::devices::HashDevice),
    )
    .await
    .unwrap();
    File::write_to_path(
        vfs,
        task,
        "/system/accounts/.seed-pending",
        b"xila-seed-v1\n",
    )
    .await
    .unwrap();
    Context {
        virtual_file_system: vfs,
        task_manager,
        users_manager,
        task,
    }
}

#[task::test]
async fn first_install_and_restarts_preserve_existing_credentials() {
    for interrupted_after_group in [false, true] {
        let context = fixture().await;
        if interrupted_after_group {
            authentication::create_group(
                &context,
                "administrator",
                Some(GroupIdentifier::new(1000)),
            )
            .await
            .unwrap();
            // The retry represents a new boot with an empty in-memory user registry.
        }
        let users = users::Manager::new();
        let boot = Context {
            users_manager: &users,
            ..context
        };
        initialize_seeded(&boot).await.unwrap();
        assert_eq!(
            authentication::authenticate_user(&boot, "administrator", "")
                .await
                .unwrap(),
            UserIdentifier::new(1000)
        );
        authentication::change_user_password(&boot, "administrator", "keep-me")
            .await
            .unwrap();
        // Simulate loss of power after accounts were saved but before clearing pending state.
        File::write_to_path(
            boot.virtual_file_system,
            boot.task,
            "/system/accounts/.seed-pending",
            b"xila-seed-v1\n",
        )
        .await
        .unwrap();
        let users = users::Manager::new();
        let reboot = Context {
            users_manager: &users,
            ..boot
        };
        initialize_seeded(&reboot).await.unwrap();
        assert!(
            authentication::authenticate_user(&reboot, "administrator", "keep-me")
                .await
                .is_ok()
        );
        assert!(
            authentication::authenticate_user(&reboot, "administrator", "")
                .await
                .is_err()
        );
        assert!(
            reboot
                .virtual_file_system
                .get_statistics(&"/system/accounts/.seed-pending")
                .await
                .is_err()
        );
        // A legacy/existing installation has no pending marker and still reloads its users.
        let users = users::Manager::new();
        let existing = Context {
            users_manager: &users,
            ..reboot
        };
        initialize_seeded(&existing).await.unwrap();
        assert!(
            authentication::authenticate_user(&existing, "administrator", "keep-me")
                .await
                .is_ok()
        );
    }
}

#[task::test]
async fn invalid_pending_state_and_conflicting_accounts_are_preserved() {
    let context = fixture().await;
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        "/system/accounts/.seed-pending",
        b"wrong-version",
    )
    .await
    .unwrap();
    assert!(initialize_seeded(&context).await.is_err());
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        "/system/accounts/.seed-pending",
        b"xila-seed-v1\n",
    )
    .await
    .unwrap();
    authentication::create_group(&context, "administrator", Some(GroupIdentifier::new(1001)))
        .await
        .unwrap();
    let users = users::Manager::new();
    let boot = Context {
        users_manager: &users,
        ..context
    };
    assert!(initialize_seeded(&boot).await.is_err());
    let mut data = vec![];
    let group = authentication::read_group_file(&boot, &mut data, "administrator")
        .await
        .unwrap();
    assert_eq!(group.get_identifier(), GroupIdentifier::new(1001));
}
