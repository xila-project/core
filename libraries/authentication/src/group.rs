// Group management functionality for the Authentication module.
//
// This module provides group management capabilities including:
// - Group creation and management
// - User group membership tracking
// - File-based persistent storage of group data
//
// All group data is stored as JSON files in the `/System/Groups/` directory,
// with each group having their own file named after their group name.

use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use file_system::{AccessFlags, Path, PathOwned};
use miniserde::{Deserialize, Serialize};
use users::{GroupIdentifier, GroupIdentifierInner, UserIdentifier, UserIdentifierInner};
use virtual_file_system::{Directory, File};

use crate::{Context, Error, GROUP_FOLDER_PATH, Result};

/// Represents a user group with associated metadata and member list.
///
/// This structure contains all the information needed to represent a group
/// in the system, including its unique identifier, name, and list of users
/// that belong to the group.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Group {
    /// Unique identifier for the group
    identifier: GroupIdentifierInner,
    /// Human-readable group name
    name: String,
    /// List of user identifiers that belong to this group
    users: Vec<UserIdentifierInner>,
}

impl Group {
    /// Creates a new group instance with the provided information.
    ///
    /// # Arguments
    ///
    /// * `Identifier` - Unique numerical identifier for the group
    /// * `Name` - Human-readable group name
    /// * `Users` - List of user identifiers that belong to this group
    ///
    /// # Returns
    ///
    /// A new `Group_type` instance with the provided data.
    pub fn new(
        identifier: GroupIdentifierInner,
        name: String,
        users: Vec<UserIdentifierInner>,
    ) -> Self {
        Self {
            identifier,
            name,
            users,
        }
    }

    /// Returns the group's unique identifier.
    ///
    /// # Returns
    ///
    /// A `GroupIdentifier` containing the group's unique ID.
    pub fn get_identifier(&self) -> GroupIdentifier {
        GroupIdentifier::new(self.identifier)
    }

    /// Returns the group's name as a string slice.
    ///
    /// # Returns
    ///
    /// A string slice containing the group name.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Returns the list of users that belong to this group.
    ///
    /// This function uses unsafe transmutation to avoid copying the vector,
    /// since `UserIdentifier` is transparent to `User_identifier_inner_type`.
    ///
    /// # Returns
    ///
    /// A slice of `UserIdentifier` containing all group members.
    pub fn get_users(&self) -> &[UserIdentifier] {
        // Avoid to copy the vector since UserIdentifier is transparent to User_identifier_inner_type.
        unsafe { core::mem::transmute(self.users.as_slice()) }
    }
}

/// Constructs the file system path for a group's data file.
///
/// # Arguments
///
/// * `Group_name` - The group name to generate a path for
///
/// # Returns
///
/// Returns `Ok(Path_owned_type)` with the complete path to the group file,
/// or `Err(Error::Failed_to_get_group_file_path)` if path construction fails.
pub fn get_group_file_path(group_name: &str) -> Result<PathOwned> {
    Path::new(GROUP_FOLDER_PATH)
        .to_owned()
        .append(group_name)
        .ok_or(Error::FailedToGetGroupFilePath)
}

/// Reads and parses a group file from the filesystem.
///
/// This function is used internally to load group data from JSON files.
/// It reads the file contents into the provided buffer and deserializes
/// the JSON data into a `Group_type` structure.
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `Buffer` - Mutable buffer to use for reading file contents
/// * `File` - Name of the group file to read
///
/// # Returns
///
/// Returns `Ok(Group_type)` with the parsed group data,
/// or an appropriate error if reading or parsing fails.
///
/// # Errors
///
/// - Path construction failures
/// - File system errors (opening, reading)
/// - JSON parsing errors
pub async fn read_group_file(
    context: &Context<'_>,
    buffer: &mut Vec<u8>,
    file: &str,
) -> Result<Group> {
    let group_file_path = Path::new(GROUP_FOLDER_PATH)
        .to_owned()
        .append(file)
        .ok_or(Error::FailedToGetGroupFilePath)?;

    let virtual_file_system = context.virtual_file_system;
    let task = context.task;

    let mut group_file = File::open(
        virtual_file_system,
        task,
        group_file_path,
        AccessFlags::Read.into(),
    )
    .await
    .map_err(Error::FailedToReadGroupDirectory)?;

    buffer.clear();

    group_file
        .read_to_end(buffer, 32)
        .await
        .map_err(Error::FailedToReadGroupFile)?;

    group_file
        .close(virtual_file_system)
        .await
        .map_err(Error::FailedToCloseFile)?;

    miniserde::json::from_str(core::str::from_utf8(buffer).unwrap())
        .map_err(Error::FailedToParseGroupFile)
}

/// Creates a new group with the specified parameters.
///
/// This function creates a new group in the system by:
/// 1. Generating a new group identifier (if not provided)
/// 2. Adding the group to the Users manager
/// 3. Creating the group file with the group data
///
/// The group is created with an empty user list initially.
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `Group_name` - Name for the new group
/// * `Group_identifier` - Optional specific group identifier (auto-generated if None)
///
/// # Returns
///
/// Returns `Ok(GroupIdentifier)` with the new group's identifier,
/// or an appropriate error if creation fails.
///
/// # Errors
///
/// This function can fail for various reasons including:
/// - Group identifier generation or assignment failures
/// - File system operations (directory creation, file writing)
/// - Users manager operations (adding group)
pub async fn create_group(
    context: &Context<'_>,
    group_name: &str,
    group_identifier: Option<GroupIdentifier>,
) -> Result<GroupIdentifier> {
    let virtual_file_system = context.virtual_file_system;
    let users_manager = context.users_manager;

    // - New group identifier if not provided.
    let group_identifier = if let Some(group_identifier) = group_identifier {
        group_identifier
    } else {
        users_manager
            .get_new_group_identifier()
            .await
            .map_err(Error::FailedToGetNewGroupIdentifier)?
    };

    // - Add it to the users manager.
    users_manager
        .add_group(group_identifier, group_name, &[])
        .await
        .map_err(Error::FailedToAddGroup)?;

    // - Write group file.
    let group = Group::new(group_identifier.as_u16(), group_name.to_string(), vec![]);

    let task = context.task;

    match Directory::create(virtual_file_system, task, GROUP_FOLDER_PATH).await {
        Ok(_) | Err(virtual_file_system::Error::AlreadyExists) => {}
        Err(error) => Err(Error::FailedToCreateGroupsDirectory(error))?,
    };

    let group_file_path = get_group_file_path(group_name)?;

    let group_json = miniserde::json::to_string(&group);

    File::write_to_path(
        virtual_file_system,
        task,
        &group_file_path,
        group_json.as_bytes(),
    )
    .await
    .map_err(Error::FailedToWriteGroupFile)?;

    Ok(group_identifier)
}
