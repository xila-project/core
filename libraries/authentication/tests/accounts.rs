#![cfg(target_os = "linux")]
use authentication::{Context, Error};
use file_system::Kind;
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, ItemStatic};
testing_support::instantiate_global_allocator!();

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
    let context = Context {
        virtual_file_system: vfs,
        task_manager,
        users_manager,
        task,
    };
    authentication::create_group(&context, "people", Some(GroupIdentifier::new(1000)))
        .await
        .unwrap();
    context
}

async fn random_password(context: &Context<'_>) -> String {
    let mut bytes = [0; 16];
    File::read_slice_from_path(
        context.virtual_file_system,
        context.task,
        "/devices/random/source",
        &mut bytes,
    )
    .await
    .unwrap();
    format!("{:032x}", u128::from_ne_bytes(bytes))
}

async fn read(context: &Context<'_>, path: &str) -> String {
    let mut bytes = Vec::new();
    File::read_from_path(context.virtual_file_system, context.task, &path, &mut bytes)
        .await
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

async fn exists(context: &Context<'_>, path: &str) -> bool {
    context
        .virtual_file_system
        .get_statistics(&path)
        .await
        .is_ok()
}

/// Reload every account into a fresh registry, as a reboot does.
async fn reload(context: &Context<'_>) -> &'static users::Manager {
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
async fn creating_a_user_records_a_home_named_after_the_user() {
    let context = fixture().await;
    let group = GroupIdentifier::new(1000);
    let password = random_password(&context).await;
    let alice = authentication::create_user(&context, "alice", &password, group, None)
        .await
        .unwrap();

    assert_eq!(
        context.users_manager.get_user_home(alice).await.unwrap(),
        "/users/alice"
    );
    let record = read(&context, "/system/accounts/users/alice").await;
    assert!(record.contains(r#""version":1"#), "{record}");
    assert!(record.contains(r#""home":"/users/alice""#), "{record}");

    // The directory is created lazily, owned by the user.
    let vfs = context.virtual_file_system;
    let home = vfs.ensure_user_home(context.task, alice).await.unwrap();
    assert_eq!(home.as_str(), "/users/alice");
    let statistics = vfs.get_statistics(&home).await.unwrap();
    assert_eq!(statistics.kind, Kind::Directory);
    assert_eq!(statistics.user, alice);

    // The recorded home survives a reboot.
    let users_manager = reload(&context).await;
    assert_eq!(
        users_manager.get_user_home(alice).await.unwrap(),
        "/users/alice"
    );
}

#[task::test]
async fn invalid_user_names_are_rejected() {
    let context = fixture().await;
    let group = GroupIdentifier::new(1000);
    let password = random_password(&context).await;
    for name in [
        "",
        ".",
        "..",
        "../root",
        "a/b",
        "a\0b",
        "a\nb",
        "with space",
        "1000",
        ".hidden",
        "thirty-three-characters-long-name",
    ] {
        assert!(
            matches!(
                authentication::create_user(&context, name, &password, group, None).await,
                Err(Error::InvalidUserName)
            ),
            "{name:?}"
        );
    }
    assert!(!exists(&context, "/system/accounts/users/a").await);
    assert!(!exists(&context, "/users/root").await);

    authentication::create_user(&context, "alice", &password, group, None)
        .await
        .unwrap();
    for name in ["", "..", "a/b", "1000"] {
        assert!(
            matches!(
                authentication::change_user_name(&context, "alice", name).await,
                Err(Error::InvalidUserName)
            ),
            "{name:?}"
        );
    }
    // Lookups cannot escape the accounts directory either.
    assert!(matches!(
        authentication::authenticate_user(&context, "../groups/people", &password).await,
        Err(Error::FailedToGetUserFilePath)
    ));
}

#[task::test]
async fn version_zero_records_fall_back_to_the_identifier_home_without_writing() {
    let context = fixture().await;
    let legacy = br#"{"identifier":42,"name":"bob","primary_group":1000,"hash":"saved-hash","salt":"saved-salt"}"#;
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        &"/system/accounts/users/bob",
        legacy,
    )
    .await
    .unwrap();
    let modification = context
        .virtual_file_system
        .get_statistics(&"/system/accounts/users/bob")
        .await
        .unwrap()
        .modification;

    let users_manager = reload(&context).await;
    let bob = UserIdentifier::new(42);
    assert_eq!(users_manager.get_user_name(bob).await.unwrap(), "bob");
    assert_eq!(users_manager.get_user_home(bob).await.unwrap(), "/users/42");
    // Loading is read-only: the record is untouched.
    assert_eq!(
        read(&context, "/system/accounts/users/bob")
            .await
            .as_bytes(),
        legacy
    );
    assert_eq!(
        context
            .virtual_file_system
            .get_statistics(&"/system/accounts/users/bob")
            .await
            .unwrap()
            .modification,
        modification
    );

    // Rewriting the record for another reason records the same home.
    let context = Context {
        users_manager,
        ..context
    };
    let password = random_password(&context).await;
    authentication::change_user_password(&context, "bob", &password)
        .await
        .unwrap();
    let record = read(&context, "/system/accounts/users/bob").await;
    assert!(record.contains(r#""version":1"#), "{record}");
    assert!(record.contains(r#""home":"/users/42""#), "{record}");
    assert_eq!(
        reload(&context).await.get_user_home(bob).await.unwrap(),
        "/users/42"
    );
    assert_eq!(
        authentication::authenticate_user(&context, "bob", &password)
            .await
            .unwrap(),
        bob
    );
}

#[task::test]
async fn unsupported_records_are_not_loaded() {
    let context = fixture().await;
    let password = random_password(&context).await;
    for (name, record) in [
        (
            "future",
            r#"{"version":2,"identifier":50,"name":"future","primary_group":1000,"home":"/users/future","hash":"h","salt":"s"}"#,
        ),
        (
            "escaped",
            r#"{"version":1,"identifier":51,"name":"escaped","primary_group":1000,"home":"/system","hash":"h","salt":"s"}"#,
        ),
    ] {
        File::write_to_path(
            context.virtual_file_system,
            context.task,
            &format!("/system/accounts/users/{name}").as_str(),
            record.as_bytes(),
        )
        .await
        .unwrap();
    }
    let users_manager = reload(&context).await;
    assert!(
        !users_manager
            .exists_user(UserIdentifier::new(50))
            .await
            .unwrap()
    );
    assert!(
        !users_manager
            .exists_user(UserIdentifier::new(51))
            .await
            .unwrap()
    );
    assert!(matches!(
        authentication::authenticate_user(&context, "future", &password).await,
        Err(Error::UnsupportedUserRecordVersion)
    ));
    assert!(matches!(
        authentication::authenticate_user(&context, "escaped", &password).await,
        Err(Error::InvalidUserHome)
    ));
}

#[task::test]
async fn renaming_keeps_the_recorded_home() {
    let context = fixture().await;
    let group = GroupIdentifier::new(1000);
    let password = random_password(&context).await;
    let alice = authentication::create_user(&context, "alice", &password, group, None)
        .await
        .unwrap();
    let bob = authentication::create_user(&context, "bob", &password, group, None)
        .await
        .unwrap();
    let vfs = context.virtual_file_system;
    let home = vfs.ensure_user_home(context.task, alice).await.unwrap();
    File::write_to_path(vfs, context.task, &"/users/alice/notes", b"kept")
        .await
        .unwrap();

    assert!(matches!(
        authentication::change_user_name(&context, "alice", "bob").await,
        Err(Error::FailedToRenameUser(users::Error::DuplicateUserName))
    ));
    authentication::change_user_name(&context, "alice", "alicia")
        .await
        .unwrap();

    assert_eq!(
        context.users_manager.get_user_name(alice).await.unwrap(),
        "alicia"
    );
    assert_eq!(
        vfs.ensure_user_home(context.task, alice).await.unwrap(),
        home
    );
    assert_eq!(read(&context, "/users/alice/notes").await, "kept");
    assert!(!exists(&context, "/system/accounts/users/alice").await);
    assert!(
        !exists(
            &context,
            &format!("/system/accounts/.user-{}.tmp", alice.as_u16())
        )
        .await
    );
    assert!(!exists(&context, "/users/alicia").await);
    assert_eq!(
        authentication::authenticate_user(&context, "alicia", &password)
            .await
            .unwrap(),
        alice
    );
    assert!(
        authentication::authenticate_user(&context, "alice", &password)
            .await
            .is_err()
    );

    // The renamed account still owns /users/alice, so the old name cannot be
    // given to a new account.
    assert!(matches!(
        authentication::create_user(&context, "alice", &password, group, None).await,
        Err(Error::UserHomeAlreadyUsed)
    ));

    let users_manager = reload(&context).await;
    assert_eq!(users_manager.get_user_name(alice).await.unwrap(), "alicia");
    assert_eq!(
        users_manager.get_user_home(alice).await.unwrap(),
        "/users/alice"
    );
    assert_eq!(
        users_manager.get_user_home(bob).await.unwrap(),
        "/users/bob"
    );
}

#[task::test]
async fn an_interrupted_rename_loads_under_the_new_name() {
    let context = fixture().await;
    let group = GroupIdentifier::new(1000);
    let password = random_password(&context).await;
    let alice = authentication::create_user(&context, "alice", &password, group, None)
        .await
        .unwrap();
    // Power loss after the record file was moved but before it was rewritten.
    context
        .virtual_file_system
        .rename(
            context.task,
            &"/system/accounts/users/alice",
            &"/system/accounts/users/alicia",
        )
        .await
        .unwrap();
    let users_manager = reload(&context).await;
    assert_eq!(users_manager.get_user_name(alice).await.unwrap(), "alicia");
    assert_eq!(
        users_manager.get_user_home(alice).await.unwrap(),
        "/users/alice"
    );
    assert_eq!(
        authentication::authenticate_user(&context, "alicia", &password)
            .await
            .unwrap(),
        alice
    );
}

#[task::test]
async fn a_home_owned_by_someone_else_is_not_reused() {
    let context = fixture().await;
    let password = random_password(&context).await;
    let vfs = context.virtual_file_system;
    vfs.create_directory(context.task, &"/users/carol")
        .await
        .unwrap();
    vfs.set_ownership(
        context.task,
        &"/users/carol",
        Some(UserIdentifier::new(4000)),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        authentication::create_user(
            &context,
            "carol",
            &password,
            GroupIdentifier::new(1000),
            Some(UserIdentifier::new(1500))
        )
        .await,
        Err(Error::UserHomeAlreadyUsed)
    ));
    assert!(
        !context
            .users_manager
            .exists_user(UserIdentifier::new(1500))
            .await
            .unwrap()
    );
}

#[task::test]
async fn a_failed_rewrite_still_commits_the_rename() {
    let context = fixture().await;
    let group = GroupIdentifier::new(1000);
    let password = random_password(&context).await;
    let alice = authentication::create_user(&context, "alice", &password, group, None)
        .await
        .unwrap();
    // A directory in place of the temporary record makes the rewrite fail.
    let vfs = context.virtual_file_system;
    let temporary = format!("/system/accounts/.user-{}.tmp", alice.as_u16());
    vfs.create_directory(context.task, &temporary.as_str())
        .await
        .unwrap();

    authentication::change_user_name(&context, "alice", "alicia")
        .await
        .unwrap();

    // The rewrite failed: the moved record still holds the old name.
    let record = read(&context, "/system/accounts/users/alicia").await;
    assert!(record.contains(r#""name":"alice""#), "{record}");
    // The registry follows the moved record, so the new name is not free.
    assert_eq!(
        context.users_manager.get_user_name(alice).await.unwrap(),
        "alicia"
    );
    assert!(matches!(
        authentication::create_user(&context, "alicia", &password, group, None).await,
        Err(Error::FailedToCreateUser(users::Error::DuplicateUserName))
    ));
    assert_eq!(
        authentication::authenticate_user(&context, "alicia", &password)
            .await
            .unwrap(),
        alice
    );
    let users_manager = reload(&context).await;
    assert_eq!(users_manager.get_user_name(alice).await.unwrap(), "alicia");
}

#[task::test]
async fn an_existing_record_is_never_overwritten() {
    let context = fixture().await;
    let password = random_password(&context).await;
    // Written by newer firmware: skipped at load time, so not registered.
    let record = r#"{"version":2,"identifier":60,"name":"dave","primary_group":1000,"home":"/users/dave","hash":"h","salt":"s"}"#;
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        &"/system/accounts/users/dave",
        record.as_bytes(),
    )
    .await
    .unwrap();

    assert!(matches!(
        authentication::create_user(
            &context,
            "dave",
            &password,
            GroupIdentifier::new(1000),
            None
        )
        .await,
        Err(Error::FailedToCreateUser(users::Error::DuplicateUserName))
    ));
    assert_eq!(read(&context, "/system/accounts/users/dave").await, record);
}

#[task::test]
async fn a_home_that_is_not_a_directory_is_not_used() {
    let context = fixture().await;
    let password = random_password(&context).await;
    let identifier = UserIdentifier::new(1600);
    let vfs = context.virtual_file_system;
    File::write_to_path(vfs, context.task, &"/users/erin", b"file")
        .await
        .unwrap();
    vfs.set_ownership(context.task, &"/users/erin", Some(identifier), None)
        .await
        .unwrap();

    assert!(matches!(
        authentication::create_user(
            &context,
            "erin",
            &password,
            GroupIdentifier::new(1000),
            Some(identifier)
        )
        .await,
        Err(Error::UserHomeAlreadyUsed)
    ));
    assert!(!context.users_manager.exists_user(identifier).await.unwrap());
}

#[task::test]
async fn an_explicit_version_zero_is_read_like_a_missing_version() {
    let context = fixture().await;
    File::write_to_path(
        context.virtual_file_system,
        context.task,
        &"/system/accounts/users/frank",
        br#"{"version":0,"identifier":43,"name":"frank","primary_group":1000,"hash":"h","salt":"s"}"#,
    )
    .await
    .unwrap();
    let users_manager = reload(&context).await;
    assert_eq!(
        users_manager
            .get_user_home(UserIdentifier::new(43))
            .await
            .unwrap(),
        "/users/43"
    );
}
