#![no_std]

extern crate alloc;

embedded_i18n::i18n!("locales");

#[cfg(feature = "building")]
mod building;
mod error;
mod format;
#[cfg(all(
    feature = "native-dev",
    any(
        all(
            any(target_arch = "x86_64", target_arch = "aarch64"),
            target_os = "linux"
        ),
        all(
            any(
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "arm",
                target_arch = "riscv32",
                target_arch = "xtensa"
            ),
            target_os = "none"
        )
    )
))]
mod native;
mod standard;
mod traits;

#[cfg(feature = "building")]
pub use building::*;
pub use error::*;
pub use file_system as exported_file_system;
pub use format::*;
#[cfg(all(
    feature = "native-dev",
    any(
        all(
            any(target_arch = "x86_64", target_arch = "aarch64"),
            target_os = "linux"
        ),
        all(
            any(
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "arm",
                target_arch = "riscv32",
                target_arch = "xtensa"
            ),
            target_os = "none"
        )
    )
))]
pub use native::NativeFormat;
#[cfg(feature = "native-dev")]
pub use native_loader;
pub use standard::*;
pub use task as exported_task;
pub use traits::*;
pub use virtual_file_system as exported_virtual_file_system;

use alloc::{string::String, vec::Vec};
use file_system::{AccessFlags, Kind, Path, Permission, Position, Statistics};
use task::{JoinHandle, SpawnerIdentifier, TaskIdentifier};
use users::UserIdentifier;
use virtual_file_system::File;

/// Check execute permissions for a user using the same owner, group and root
/// rules as [`execute`]. This checks permissions only; it does not open the file
/// or validate its executable format.
pub async fn is_execute_allowed(
    users_manager: &users::Manager,
    statistics: &Statistics,
    user: UserIdentifier,
) -> bool {
    if statistics
        .permissions
        .get_others()
        .contains(Permission::Execute)
    {
        return true;
    }

    let is_user_allowed = user == UserIdentifier::ROOT || user == statistics.user;
    if is_user_allowed
        && statistics
            .permissions
            .get_user()
            .contains(Permission::Execute)
    {
        return true;
    }

    let is_in_group =
        users_manager.is_in_group(user, statistics.group).await || user == UserIdentifier::ROOT;
    is_in_group
        && statistics
            .permissions
            .get_group()
            .contains(Permission::Execute)
}

fn get_overridden_user(
    statistics: &Statistics,
    current_user: UserIdentifier,
) -> Result<Option<UserIdentifier>> {
    if !statistics
        .permissions
        .get_special()
        .get_set_user_identifier()
    {
        return Ok(None);
    }

    let new_user = statistics.user;

    if current_user != users::UserIdentifier::ROOT || new_user != current_user {
        return Err(Error::PermissionDenied);
    }

    Ok(Some(new_user))
}

pub async fn execute(
    context: &'static ExecutableContext,
    path: impl AsRef<Path>,
    inputs: Vec<String>,
    standard: Standard<'static>,
    spawner: Option<SpawnerIdentifier>,
) -> Result<JoinHandle<isize>> {
    execute_with(context, path, inputs, standard, spawner, Launch::default()).await
}

/// Like [`execute`], with launch parameters: an explicit environment for the new task
/// (any executable) and a home capability (native executables). The new task is a
/// child of the calling task and starts with a copy of its environment otherwise.
pub async fn execute_with(
    context: &'static ExecutableContext,
    path: impl AsRef<Path>,
    inputs: Vec<String>,
    standard: Standard<'static>,
    spawner: Option<SpawnerIdentifier>,
    launch: Launch,
) -> Result<JoinHandle<isize>> {
    let (handle, _) =
        execute_inner(context, path.as_ref(), inputs, standard, spawner, launch).await?;
    Ok(handle)
}

/// Like [`execute_with`], also returning the new task's identifier.
async fn execute_inner(
    context: &'static ExecutableContext,
    path: &Path,
    inputs: Vec<String>,
    standard: Standard<'static>,
    spawner: Option<SpawnerIdentifier>,
    launch: Launch,
) -> Result<(JoinHandle<isize>, TaskIdentifier)> {
    // Admission barrier: once a lifecycle request is accepted, no new
    // executable may start and race the shutdown pipeline.
    if !lifecycle::SYSTEM.is_running() {
        return Err(virtual_file_system::IoErrorKind::ResourceBusy.into());
    }
    let task_instance = &context.task_manager;

    let task = task_instance.try_get_current_task_identifier().await?;

    let virtual_file_system = &context.virtual_file_system;

    // Query by path: mounted executables do not implement attribute operations on their handles.
    let statistics = virtual_file_system.get_statistics(&path).await?;

    // The program runs as the requested identity, if any, so it is the one that
    // needs the execute permission.
    let identity = launch.identity;
    let user = match identity {
        Some(identity) => identity.user,
        None => task_instance.get_user(task).await?,
    };

    // - Check the executable bit
    if !is_execute_allowed(context.users_manager, &statistics, user).await {
        return Err(Error::PermissionDenied);
    }

    let mut file = File::open(virtual_file_system, task, path, AccessFlags::Read.into()).await?;

    let file_name = path
        .get_file_name()
        .ok_or(virtual_file_system::IoErrorKind::InvalidPath)?;

    if statistics.kind == Kind::File {
        let mut magic = [0; 4];
        let mut read = 0;
        while read < magic.len() {
            let count = file.read(&mut magic[read..]).await?;
            if count == 0 {
                break;
            }
            read += count;
        }
        if read != 0 {
            file.set_position(&Position::Start(0)).await?;
        }
        if read == magic.len()
            && let Some(handler) = find_format(
                context.formats,
                magic,
                statistics
                    .permissions
                    .get_special()
                    .get_set_user_identifier(),
            )?
        {
            // Format handlers do not apply identities (see `Launch`): refuse instead
            // of running the program with the caller's identity.
            if identity.is_some() {
                return Err(Error::PermissionDenied);
            }
            let mut standard = standard;
            standard.context = context;
            return handler
                .launch(FormatRequest {
                    file,
                    file_size: statistics.size,
                    name: String::from(file_name),
                    parent: task,
                    spawner,
                    standard,
                    arguments: inputs,
                    launch,
                })
                .await;
        }
    }

    let new_user = get_overridden_user(&statistics, user)?;

    let main_function: MainFunction = file.control(GET_MAIN_FUNCTION, &()).await?;

    let main = main_function.ok_or(Error::FailedToGetMainFunction)?;

    let spawned = task_instance
        .spawn(task, file_name, spawner, async move |task| {
            // The identity is set before the program starts, on its own task only.
            // The task was registered a moment ago, so this cannot fail; if it ever
            // did, the program must not run with an identity it was not given.
            if let Some(identity) = identity
                && (context
                    .task_manager
                    .set_user(task, identity.user)
                    .await
                    .is_err()
                    || context
                        .task_manager
                        .set_group(task, identity.group)
                        .await
                        .is_err())
            {
                return -1;
            }
            if let Some(new_user) = new_user {
                context.task_manager.set_user(task, new_user).await.unwrap();
            }
            if let Some(environment) = launch.environment {
                // The task was registered a moment ago, so replacement cannot fail.
                let _ = context
                    .task_manager
                    .replace_environment_variables(task, environment)
                    .await;
            }

            match main(context, standard, inputs).await {
                Ok(_) => 0_isize,
                Err(error) => -(error.get() as isize),
            }
        })
        .await?;

    Ok(spawned)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use file_system::{Permissions, Time};

    use task::test;
    use users::GroupIdentifier;

    use super::*;

    fn get_statistics_with_permissions(permissions: Permissions) -> Statistics {
        Statistics::new(
            0,
            1,
            0,
            Time::new(0),
            Time::new(0),
            Time::new(0),
            Time::new(0),
            file_system::Kind::File,
            permissions,
            UserIdentifier::ROOT,
            GroupIdentifier::ROOT,
        )
    }

    #[test]
    async fn test_is_execute_allowed() {
        let users_manager = users::Manager::new();

        let statistics = get_statistics_with_permissions(Permissions::ALL_FULL);
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::EXECUTABLE);
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o007).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o070).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o100).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::USER_READ_WRITE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::NONE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::ALL_READ_WRITE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    struct Handler;
    impl FormatHandler for Handler {
        fn launch(&self, _: FormatRequest) -> FormatLaunch<'_> {
            unreachable!()
        }
    }

    static FORMATS: [ExecutableFormat; 2] = [
        ExecutableFormat::new(*b"\0asm", &Handler),
        ExecutableFormat::new(*b"\x7fELF", &Handler),
    ];

    #[test]
    fn detects_registered_formats_and_applies_native_policy() {
        assert!(find_format(&FORMATS, *b"\0asm", false).unwrap().is_some());
        assert!(find_format(&FORMATS, *b"data", false).unwrap().is_none());
        assert!(matches!(
            find_format(&FORMATS, *b"\x7fELF", true),
            Err(Error::PermissionDenied)
        ));
        assert!(matches!(
            find_format(&[], *b"\x7fELF", false),
            Err(Error::PermissionDenied)
        ));
        let native = find_format(&FORMATS, *b"\x7fELF", false);
        if cfg!(feature = "native-dev") {
            assert!(native.unwrap().is_some());
        } else {
            assert!(matches!(native, Err(Error::PermissionDenied)));
        }
    }
}
