//! # Authentication Module
//!
//! The Authentication module provides user and group management functionality for the Xila operating system.
//! It handles user authentication, password hashing, and secure storage of user/group information.
//!
//! ## Features
//!
//! - **User Management**: Create, authenticate, and manage user accounts
//! - **Group Management**: Create and manage user groups
//! - **Password Security**: Secure password hashing using SHA-512 with salt
//! - **File-based Storage**: Persistent storage of user and group data in JSON format
//! - **Async Operations**: All operations are asynchronous for better performance
//!
//! ## Usage
//!
//! The module provides functionality to:
//! - Load existing users and groups from the filesystem
//! - Authenticate users with username/password
//! - Create new users and groups
//! - Change user passwords and usernames
//! - Hash passwords securely with salt generation
//!
//! ## File Structure
//!
//! - Users are stored in `/System/Users/` directory
//! - Groups are stored in `/System/Groups/` directory
//! - Each user/group has their own JSON file containing their data
//! - Random salt generation uses `/devices/random/source`

#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

mod error;
mod seed;
pub use seed::{initialize_seeded, SEED_PENDING_CONTENT, SEED_PENDING_PATH};
mod group;
mod hash;
mod user;

use alloc::{vec, vec::Vec};
pub use error::*;
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
/// This function scans the `/System/Users` and `/System/Groups` directories,
/// reads all user and group files, and adds them to the Users manager.
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
        for group_entry in groups_directory {
            let group =
                if let Ok(group) = read_group_file(context, &mut buffer, &group_entry.name).await {
                    group
                } else {
                    // ? : Log error ?
                    continue;
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
        for user_entry in users_directory {
            let user =
                if let Ok(user) = read_user_file(context, &mut buffer, &user_entry.name).await {
                    user
                } else {
                    // ? : Log error ?
                    continue;
                };

            users_manager
                .add_user(
                    user.get_identifier(),
                    user.get_name(),
                    user.get_primary_group(),
                )
                .await
                .map_err(Error::FailedToAddUser)?;
        }
    }

    Ok(())
}
