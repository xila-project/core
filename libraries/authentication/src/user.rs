//! User management functionality for the Authentication module.

//!
//! This module provides comprehensive user account management including:
//! - User creation and authentication
//! - Password management with secure hashing
//! - User profile management (name, primary group)
//! - File-based persistent storage of user data
//!
//! All user data is stored as JSON files in the `/System/Users/` directory,
//! with each user having their own file named after their username.

use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec::Vec,
};
use file_system::{AccessFlags, Path, PathOwned};
use miniserde::{Deserialize, Serialize};
use users::{GroupIdentifier, GroupIdentifierInner, UserIdentifier, UserIdentifierInner};
use virtual_file_system::{Directory, File};

use crate::{
    Context, Error, READ_CHUNK_SIZE, Result, USERS_FOLDER_PATH,
    hash::{generate_salt, hash_password},
};

/// Represents a user account with all associated metadata.
///
/// This structure contains all the information needed to represent a user
/// in the system, including their unique identifier, name, primary group,
/// and hashed password with salt for security.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    /// Unique identifier for the user
    identifier: UserIdentifierInner,
    /// Human-readable username
    name: String,
    /// Identifier of the user's primary group
    primary_group: GroupIdentifierInner,
    /// SHA-512 hash of the user's password combined with salt
    hash: String,
    /// Random salt used for password hashing
    salt: String,
}

impl User {
    /// Creates a new user instance with the provided information.
    ///
    /// # Arguments
    ///
    /// * `Identifier` - Unique numerical identifier for the user
    /// * `Name` - Human-readable username
    /// * `Primary_group` - Identifier of the user's primary group
    /// * `Hash` - Pre-computed SHA-512 hash of password+salt
    /// * `Salt` - Random salt used for password hashing
    ///
    /// # Returns
    ///
    /// A new `User_type` instance with the provided data.
    pub fn new(
        identifier: UserIdentifierInner,
        name: String,
        primary_group: GroupIdentifierInner,
        hash: String,
        salt: String,
    ) -> Self {
        Self {
            identifier,
            name,
            primary_group,
            hash,
            salt,
        }
    }

    /// Returns the user's unique identifier.
    ///
    /// # Returns
    ///
    /// A `UserIdentifier` containing the user's unique ID.
    pub fn get_identifier(&self) -> UserIdentifier {
        UserIdentifier::new(self.identifier)
    }

    /// Returns the user's primary group identifier.
    ///
    /// # Returns
    ///
    /// A `GroupIdentifier` containing the user's primary group ID.
    pub fn get_primary_group(&self) -> GroupIdentifier {
        GroupIdentifier::new(self.primary_group)
    }

    /// Returns the user's name as a string slice.
    ///
    /// # Returns
    ///
    /// A string slice containing the username.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Returns the user's password hash as a string slice.
    ///
    /// # Returns
    ///
    /// A string slice containing the SHA-512 hash of password+salt.
    pub fn get_hash(&self) -> &str {
        &self.hash
    }

    /// Returns the user's salt as a string slice.
    ///
    /// # Returns
    ///
    /// A string slice containing the random salt used for password hashing.
    pub fn get_salt(&self) -> &str {
        &self.salt
    }

    /// Updates the user's password hash.
    ///
    /// # Arguments
    ///
    /// * `Hash` - New SHA-512 hash to store
    pub fn set_hash(&mut self, hash: String) {
        self.hash = hash;
    }

    /// Updates the user's salt.
    ///
    /// # Arguments
    ///
    /// * `Salt` - New salt to store
    pub fn set_salt(&mut self, salt: String) {
        self.salt = salt;
    }

    /// Updates the user's primary group.
    ///
    /// # Arguments
    ///
    /// * `Primary_group` - New primary group identifier
    pub fn set_primary_group(&mut self, primary_group: GroupIdentifierInner) {
        self.primary_group = primary_group;
    }

    /// Updates the user's name.
    ///
    /// # Arguments
    ///
    /// * `Name` - New username to store
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

/// Constructs the file system path for a user's data file.
///
/// # Arguments
///
/// * `User_name` - The username to generate a path for
///
/// # Returns
///
/// Returns `Ok(Path_owned_type)` with the complete path to the user file,
/// or `Err(Error::Failed_to_get_user_file_path)` if path construction fails.
pub fn get_user_file_path(user_name: &str) -> Result<PathOwned> {
    Path::new(USERS_FOLDER_PATH)
        .to_owned()
        .append(user_name)
        .ok_or(Error::FailedToGetUserFilePath)
}

/// Authenticates a user with their username and password.
///
/// This function reads the user's file from the filesystem, compares the
/// provided password hash with the stored hash, and returns the user's
/// identifier if authentication succeeds.
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `User_name` - Username to authenticate
/// * `Password` - Plain text password to verify
///
/// # Returns
///
/// Returns `Ok(UserIdentifier)` if authentication succeeds,
/// or an appropriate error if authentication fails or file operations fail.
///
/// # Errors
///
/// - `Failed_to_get_user_file_path` - Invalid username or path construction failure
/// - `Failed_to_open_user_file` - User file doesn't exist or permission denied
/// - `Failed_to_read_user_file` - I/O error reading user file
/// - `Failed_to_parse_user_file` - Invalid JSON format in user file
/// - `Invalid_password` - Password doesn't match stored hash
pub async fn authenticate_user(
    context: &Context<'_>,
    user_name: &str,
    password: &str,
) -> Result<UserIdentifier> {
    let path = get_user_file_path(user_name)?;

    let virtual_file_system = context.virtual_file_system;
    let task = context.task;

    let mut user_file = File::open(virtual_file_system, task, path, AccessFlags::Read.into())
        .await
        .map_err(Error::FailedToOpenUserFile)?;

    let mut buffer = Vec::new();

    user_file
        .read_to_end(&mut buffer, READ_CHUNK_SIZE)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    user_file
        .close(virtual_file_system)
        .await
        .map_err(Error::FailedToCloseFile)?;

    let user: User = miniserde::json::from_str(core::str::from_utf8(&buffer).unwrap())
        .map_err(Error::FailedToParseUserFile)?;

    let hashed_password =
        hash_password(virtual_file_system, task, password, user.get_salt()).await?;

    if hashed_password == user.get_hash() {
        Ok(user.get_identifier())
    } else {
        Err(Error::InvalidPassword)
    }
}

/// Creates a new user account with the specified parameters.
///
/// This function creates a new user in the system by:
/// 1. Generating a new user identifier (if not provided)
/// 2. Adding the user to the Users manager
/// 3. Generating a random salt and hashing the password
/// 4. Creating the user file with all user data
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `User_name` - Username for the new account
/// * `Password` - Plain text password for the new account
/// * `Primary_group` - Primary group identifier for the user
/// * `User_identifier` - Optional specific user identifier (auto-generated if None)
///
/// # Returns
///
/// Returns `Ok(UserIdentifier)` with the new user's identifier,
/// or an appropriate error if creation fails.
///
/// # Errors
///
/// This function can fail for various reasons including:
/// - User identifier generation or assignment failures
/// - File system operations (directory creation, file writing)
/// - Users manager operations (adding user)
/// - Random salt generation failures
pub async fn create_user(
    context: &Context<'_>,
    user_name: &str,
    password: &str,
    primary_group: GroupIdentifier,
    user_identifier: Option<UserIdentifier>,
) -> Result<UserIdentifier> {
    let virtual_file_system = context.virtual_file_system;
    let users_manager = context.users_manager;

    // - New user identifier if not provided.
    let user_identifier = if let Some(user_identifier) = user_identifier {
        user_identifier
    } else {
        users_manager
            .get_new_user_identifier()
            .await
            .map_err(Error::FailedToGetNewUserIdentifier)?
    };

    // - Add it to the users manager.
    users_manager
        .add_user(user_identifier, user_name, primary_group)
        .await
        .map_err(Error::FailedToCreateUser)?;

    // - Hash password.
    let task = context.task;

    let salt = generate_salt(virtual_file_system, task).await?;

    let hash = hash_password(virtual_file_system, task, password, &salt).await?;

    // - Write user file.
    let user = User::new(
        user_identifier.as_u16(),
        user_name.to_string(),
        primary_group.as_u16(),
        hash,
        salt,
    );

    let task = context.task;

    match Directory::create(virtual_file_system, task, USERS_FOLDER_PATH).await {
        Ok(_) | Err(virtual_file_system::Error::AlreadyExists) => {}
        Err(error) => Err(Error::FailedToCreateUsersDirectory(error))?,
    }

    let user_file_path = Path::new(USERS_FOLDER_PATH)
        .to_owned()
        .append(user_name)
        .ok_or(Error::FailedToGetUserFilePath)?;

    let user_json = miniserde::json::to_string(&user);

    File::write_to_path(
        virtual_file_system,
        task,
        &user_file_path,
        user_json.as_bytes(),
    )
    .await
    .map_err(Error::FailedToWriteUserFile)?;

    Ok(user_identifier)
}

/// Changes a user's password by generating a new salt and hash.
///
/// This function updates a user's password by:
/// 1. Generating a new random salt
/// 2. Hashing the new password with the salt
/// 3. Reading the existing user file
/// 4. Updating the hash and salt fields
/// 5. Writing the updated data back to the file
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `User_name` - Username of the account to update
/// * `New_password` - New plain text password
///
/// # Returns
///
/// Returns `Ok(())` if the password was changed successfully,
/// or an appropriate error if the operation fails.
///
/// # Errors
///
/// - File system errors (opening, reading, writing user file)
/// - Salt generation failures
/// - JSON parsing errors
pub async fn change_user_password(
    context: &Context<'_>,
    user_name: &str,
    new_password: &str,
) -> Result<()> {
    let virtual_file_system = context.virtual_file_system;
    let task = context.task;

    let salt = generate_salt(virtual_file_system, task).await?;

    let hash = hash_password(virtual_file_system, task, new_password, &salt).await?;

    let user_file_path = Path::new(USERS_FOLDER_PATH)
        .to_owned()
        .append(user_name)
        .ok_or(Error::FailedToGetUserFilePath)?;

    let mut buffer = Vec::new();

    File::read_from_path(virtual_file_system, task, &user_file_path, &mut buffer)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    let mut user: User =
        miniserde::json::from_str(core::str::from_utf8(buffer.as_slice()).unwrap())
            .map_err(Error::FailedToParseUserFile)?;

    user.set_hash(hash);
    user.set_salt(salt);

    let user_json = miniserde::json::to_string(&user);

    File::write_to_path(
        virtual_file_system,
        task,
        &user_file_path,
        user_json.as_bytes(),
    )
    .await
    .map_err(Error::FailedToWriteUserFile)?;

    Ok(())
}

/// Changes a user's username by updating their user file.
///
/// This function reads the user's existing data, updates the name field,
/// and writes the modified data back to the file system.
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `Current_name` - Current username of the account
/// * `New_name` - New username to assign
///
/// # Returns
///
/// Returns `Ok(())` if the username was changed successfully,
/// or an appropriate error if the operation fails.
///
/// # Errors
///
/// - File system errors (opening, reading, writing user file)
/// - JSON parsing errors
/// - Path construction failures
pub async fn change_user_name(
    context: &Context<'_>,
    current_name: &str,
    new_name: &str,
) -> Result<()> {
    let virtual_file_system = context.virtual_file_system;
    let file_path = get_user_file_path(current_name)?;

    let mut buffer = Vec::new();

    File::read_from_path(virtual_file_system, context.task, &file_path, &mut buffer)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    let mut user: User =
        miniserde::json::from_str(core::str::from_utf8(buffer.as_slice()).unwrap())
            .map_err(Error::FailedToParseUserFile)?;

    user.set_name(new_name.to_string());

    let user_json = miniserde::json::to_string(&user);

    File::write_to_path(
        virtual_file_system,
        context.task,
        get_user_file_path(new_name)?,
        user_json.as_bytes(),
    )
    .await
    .map_err(Error::FailedToWriteUserFile)?;

    Ok(())
}

/// Reads and parses a user file from the filesystem.
///
/// This function is used internally to load user data from JSON files.
/// It reads the file contents into the provided buffer and deserializes
/// the JSON data into a `User_type` structure.
///
/// # Arguments
///
/// * `Virtual_file_system` - Reference to the virtual file system
/// * `Buffer` - Mutable buffer to use for reading file contents
/// * `File` - Name of the user file to read
///
/// # Returns
///
/// Returns `Ok(User_type)` with the parsed user data,
/// or an appropriate error if reading or parsing fails.
///
/// # Errors
///
/// - Path construction failures
/// - File system errors (opening, reading)
/// - JSON parsing errors
pub async fn read_user_file(
    context: &Context<'_>,
    buffer: &mut Vec<u8>,
    file: &str,
) -> Result<User> {
    let user_file_path = get_user_file_path(file)?;

    let virtual_file_system = context.virtual_file_system;
    let task = context.task;

    File::read_from_path(virtual_file_system, task, &user_file_path, buffer)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    miniserde::json::from_str(core::str::from_utf8(buffer.as_slice()).unwrap())
        .map_err(Error::FailedToParseUserFile)
}
