//! # Authentication Module
//!
//! The Authentication module provides user and group management functionality for the Xila operating system.
//! It handles user authentication, password hashing, and secure storage of user/group information.
//!
//! ## Features
//!
//! - **User Management**: Create, authenticate, rename and manage user accounts
//! - **Group Management**: Create and manage user groups
//! - **Password Security**: Secure password hashing using SHA-512 with salt
//! - **File-based Storage**: Persistent storage of user and group data in JSON format
//! - **Async Operations**: All operations are asynchronous for better performance
//!
//! ## File Structure
//!
//! - Users are stored in `/system/accounts/users/`, one record per user, in a
//!   file named after the user
//! - Groups are stored in `/system/accounts/groups/`
//! - Random salt generation uses `/devices/random/source`
//!
//! ## User names
//!
//! New and renamed accounts must satisfy [`users::is_valid_user_name`]: an
//! ASCII letter followed by at most 31 ASCII letters, digits, `_` or `-`.
//! Existing accounts load whatever their name, but record lookups always
//! reject names that are not a single ordinary directory entry.
//!
//! ## Home directories
//!
//! Like the home field of `/etc/passwd`, each account records its home. It is
//! set to `/users/<user name>` when the account is created and read from the
//! account afterwards (`VirtualFileSystem::get_user_home`), never recomputed
//! from the current name. The directory itself is created at the first login.
//! The built-in root account has no record; its home is [`users::ROOT_HOME`].
//!
//! Renaming an account ([`change_user_name`]) keeps its recorded home, as Linux
//! `usermod --login` does without `--move-home`. Moving a home is a separate,
//! explicit operation that Xila does not perform implicitly.
//!
//! ## Account record versions
//!
//! Records carry a `version` field ([`USER_RECORD_VERSION`]). Version 1 adds
//! the `home` field. Records without a `version` are version 0, written before
//! homes were recorded: their home is `/users/<identifier>`, the path they
//! already used. That fallback is applied in memory when the record is read,
//! so booting never writes to storage; a version 0 record is upgraded only when
//! it is rewritten anyway (password change or rename), and then records the
//! same `/users/<identifier>` home. Records of an unknown version, or version 1
//! records without a valid home, are rejected. Firmware older than version 1
//! ignores the new fields and falls back to `/users/<identifier>`.

#![no_std]

extern crate alloc;

embedded_i18n::i18n!("locales");

mod error;
mod seed;
pub use seed::{SEED_PENDING_CONTENT, SEED_PENDING_PATH, initialize_seeded};
mod group;
mod hash;
mod user;

use alloc::{vec, vec::Vec};
pub use error::*;
use file_system::{AccessFlags, CreateFlags, Entry, Flags, Kind, Permissions};
pub use group::*;
use task::TaskIdentifier;
pub use user::*;
use virtual_file_system::VirtualFileSystem;

const READ_CHUNK_SIZE: usize = 32;

/// Path to the users directory in the filesystem
const USERS_FOLDER_PATH: &str = "/system/accounts/users";

/// Path to the groups directory in the filesystem
const GROUP_FOLDER_PATH: &str = "/system/accounts/groups";

/// Path to the random device used for salt generation
const RANDOM_DEVICE_PATH: &str = "/devices/random/source";

/// The dependencies required by authentication operations.
pub struct Context<'a> {
    pub virtual_file_system: &'a VirtualFileSystem,
    pub task_manager: &'a task::Manager,
    pub users_manager: &'a users::Manager,
    pub task: TaskIdentifier,
}

/// Loads all users and groups from the filesystem into memory.
///
/// This function scans the `/system/accounts/groups` and
/// `/system/accounts/users` directories, reads all group and user files, and
/// adds them to the Users manager with their recorded homes. It only reads:
/// version 0 user records get their `/users/<identifier>` home in memory.
/// It should be called during system initialization.
///
/// # Returns
///
/// Returns `Ok(())` if all users and groups were loaded successfully,
/// or an `Error` if any operation failed.
///
/// # Errors
///
/// This function can return errors in the following cases:
/// - Failed to read users or groups directory
/// - Failed to parse user or group files
/// - Failed to add users or groups to the Users manager
///
/// # Examples
///
/// ```rust
/// use authentication::load_all_users_and_groups;
///
/// async fn example(context: &authentication::Context<'_>) -> Result<(), authentication::Error> {
///     load_all_users_and_groups(context).await?;
///     Ok(())
/// }
/// ```
pub async fn load_all_users_and_groups(context: &Context<'_>) -> Result<()> {
    use group::read_group_file;
    use user::read_user_file;
    use virtual_file_system::Directory;
    // Open Xila users folder.
    let virtual_file_system = context.virtual_file_system;
    let users_manager = context.users_manager;

    let mut buffer: Vec<u8> = vec![];

    {
        let task = context.task;

        let groups_directory = Directory::open(virtual_file_system, task, GROUP_FOLDER_PATH)
            .await
            .map_err(Error::FailedToReadGroupDirectory)?;

        // Read all groups.
        for group_entry in groups_directory.filter(is_record) {
            let group = match read_group_file(context, &mut buffer, &group_entry.name).await {
                Ok(group) => group,
                Err(error) => {
                    log::warning!("Skipped a group record that cannot be loaded: {error}");
                    continue;
                }
            };

            users_manager
                .add_group(group.get_identifier(), group.get_name(), group.get_users())
                .await
                .map_err(Error::FailedToAddGroup)?;
        }
    }

    {
        let task = context.task;

        let users_directory = Directory::open(virtual_file_system, task, USERS_FOLDER_PATH)
            .await
            .map_err(Error::FailedToReadUsersDirectory)?;

        // Read all users.
        for user_entry in users_directory.filter(is_record) {
            let user = match read_user_file(context, &mut buffer, &user_entry.name).await {
                Ok(user) => user,
                Err(error) => {
                    // The name is not logged: it is personal data.
                    log::warning!("Skipped an account record that cannot be loaded: {error}");
                    continue;
                }
            };

            users_manager
                .add_user(
                    user.get_identifier(),
                    user.get_name(),
                    user.get_primary_group(),
                    user.get_home(),
                )
                .await
                .map_err(Error::FailedToAddUser)?;
        }
    }

    Ok(())
}

/// Whether a directory entry can be an account or group record: a regular
/// file. Directory iterators also report `.` and `..`, and a stray directory
/// is not a record either.
fn is_record(entry: &Entry) -> bool {
    entry.kind == Kind::File && !matches!(entry.name.as_str(), "." | "..")
}

/// Write `contents` to `temporary`, then rename it over `destination`, so the
/// destination holds either its previous or its new contents.
pub(crate) async fn atomic_write(
    context: &Context<'_>,
    temporary: &str,
    destination: &str,
    contents: &[u8],
) -> Result<()> {
    use virtual_file_system::File;

    let vfs = context.virtual_file_system;
    let mut file = File::open(
        vfs,
        context.task,
        temporary,
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .map_err(Error::FailedToWriteUserFile)?;
    file.set_permissions(Permissions::USER_READ_WRITE)
        .await
        .map_err(Error::FailedToWriteUserFile)?;
    let mut offset = 0;
    while offset < contents.len() {
        let count = file
            .write(&contents[offset..])
            .await
            .map_err(Error::FailedToWriteUserFile)?;
        if count == 0 {
            return Err(seed::invalid());
        }
        offset += count;
    }
    file.flush().await.map_err(Error::FailedToWriteUserFile)?;
    file.close(vfs).await.map_err(Error::FailedToCloseFile)?;
    vfs.rename(context.task, &temporary, &destination)
        .await
        .map_err(Error::FailedToWriteUserFile)
}
