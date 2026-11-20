//! User management functionality for the Authentication module.
//!
//! This module provides user account management including:
//! - User creation and authentication
//! - Password management with secure hashing
//! - User profile management (name, primary group, recorded home)
//! - File-based persistent storage of user data
//!
//! Each account is a JSON record in `/system/accounts/users/`, in a file
//! named after the user. See the crate documentation for the record format,
//! home directories and renaming.

use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec::Vec,
};
use file_system::{AccessFlags, Kind, Path, PathOwned};
use miniserde::{Deserialize, Serialize};
use users::{GroupIdentifier, GroupIdentifierInner, UserIdentifier, UserIdentifierInner};
use virtual_file_system::{Directory, File};

use crate::{
    Context, Error, READ_CHUNK_SIZE, Result, USERS_FOLDER_PATH, atomic_write,
    hash::{generate_salt, hash_password},
};

/// Version written to every account record.
///
/// Version 1 records the account's home. Records without a `version` field
/// predate it and are read as version 0, whose home is `/users/<identifier>`.
pub const USER_RECORD_VERSION: u8 = 1;

/// Temporary file used to replace an account record atomically.
///
/// It is named after the account's identifier, so concurrent rewrites of
/// different accounts never share it.
fn temporary_record_path(identifier: UserIdentifier) -> String {
    format!("/system/accounts/.user-{}.tmp", identifier.as_u16())
}

/// Represents a user account with all associated metadata.
///
/// This structure contains all the information needed to represent a user
/// in the system, including their unique identifier, name, primary group,
/// recorded home and hashed password with salt for security.
#[derive(Clone, Debug, Serialize)]
pub struct User {
    /// Account record format, always [`USER_RECORD_VERSION`] once loaded
    version: u8,
    /// Unique identifier for the user
    identifier: UserIdentifierInner,
    /// Human-readable username
    name: String,
    /// Identifier of the user's primary group
    primary_group: GroupIdentifierInner,
    /// Home directory recorded when the account was created
    home: String,
    /// SHA-512 hash of the user's password combined with salt
    hash: String,
    /// Random salt used for password hashing
    salt: String,
}

/// On-disk account record of any supported version.
#[derive(Deserialize)]
struct UserRecord {
    version: Option<u8>,
    identifier: UserIdentifierInner,
    name: String,
    primary_group: GroupIdentifierInner,
    home: Option<String>,
    hash: String,
    salt: String,
}

impl UserRecord {
    fn into_user(self) -> Result<User> {
        let home = match (self.version, self.home) {
            // Version 0 never recorded a home: its home was derived from the
            // identifier. This fallback happens in memory only.
            (None | Some(0), None) => legacy_user_home(UserIdentifier::new(self.identifier)),
            (Some(USER_RECORD_VERSION), Some(home)) if users::is_valid_user_home(&home) => home,
            (None | Some(0), Some(_)) | (Some(USER_RECORD_VERSION), _) => {
                return Err(Error::InvalidUserHome);
            }
            (Some(_), _) => return Err(Error::UnsupportedUserRecordVersion),
        };
        Ok(User {
            version: USER_RECORD_VERSION,
            identifier: self.identifier,
            name: self.name,
            primary_group: self.primary_group,
            home,
            hash: self.hash,
            salt: self.salt,
        })
    }
}

impl User {
    /// Creates a new user instance with the provided information.
    ///
    /// # Arguments
    ///
    /// * `identifier` - Unique numerical identifier for the user
    /// * `name` - Human-readable username
    /// * `primary_group` - Identifier of the user's primary group
    /// * `home` - Home directory recorded for the account
    /// * `hash` - Pre-computed SHA-512 hash of password+salt
    /// * `salt` - Random salt used for password hashing
    pub fn new(
        identifier: UserIdentifierInner,
        name: String,
        primary_group: GroupIdentifierInner,
        home: String,
        hash: String,
        salt: String,
    ) -> Self {
        Self {
            version: USER_RECORD_VERSION,
            identifier,
            name,
            primary_group,
            home,
            hash,
            salt,
        }
    }

    /// Returns the user's unique identifier.
    pub fn get_identifier(&self) -> UserIdentifier {
        UserIdentifier::new(self.identifier)
    }

    /// Returns the user's primary group identifier.
    pub fn get_primary_group(&self) -> GroupIdentifier {
        GroupIdentifier::new(self.primary_group)
    }

    /// Returns the user's name as a string slice.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Returns the home directory recorded in the account.
    pub fn get_home(&self) -> &str {
        &self.home
    }

    /// Returns the user's password hash as a string slice.
    pub fn get_hash(&self) -> &str {
        &self.hash
    }

    /// Returns the user's salt as a string slice.
    pub fn get_salt(&self) -> &str {
        &self.salt
    }

    /// Updates the user's password hash.
    pub fn set_hash(&mut self, hash: String) {
        self.hash = hash;
    }

    /// Updates the user's salt.
    pub fn set_salt(&mut self, salt: String) {
        self.salt = salt;
    }

    /// Updates the user's primary group.
    pub fn set_primary_group(&mut self, primary_group: GroupIdentifierInner) {
        self.primary_group = primary_group;
    }

    /// Updates the user's name. The recorded home is not changed.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

/// Home recorded for a new account: `/users/<user name>`.
///
/// Returns [`Error::InvalidUserName`] unless `user_name` satisfies
/// [`users::is_valid_user_name`].
pub fn new_user_home(user_name: &str) -> Result<String> {
    if !users::is_valid_user_name(user_name) {
        return Err(Error::InvalidUserName);
    }
    Ok(format!("{}/{user_name}", Path::USERS.as_str()))
}

/// Home of an account record saved before homes were recorded.
fn legacy_user_home(identifier: UserIdentifier) -> String {
    format!("{}/{}", Path::USERS.as_str(), identifier.as_u16())
}

/// Parses an account record read from the file named `file_name`.
///
/// The file name is authoritative for the user name: a rename moves the record
/// file first, so an interrupted rename leaves the previous name inside it.
fn parse_user_record(buffer: &[u8], file_name: &str) -> Result<User> {
    let text = core::str::from_utf8(buffer).map_err(|_| Error::InvalidUserRecord)?;
    let mut user = miniserde::json::from_str::<UserRecord>(text)
        .map_err(Error::FailedToParseUserFile)?
        .into_user()?;
    if user.name != file_name {
        user.name = file_name.to_string();
    }
    Ok(user)
}

/// Constructs the file system path for a user's data file.
///
/// Lookups accept any name that is a single, ordinary directory entry, so
/// accounts created before [`users::is_valid_user_name`] still resolve, while
/// `/`, `.`, `..`, NUL and control characters can never leave the accounts
/// directory.
///
/// # Errors
///
/// Returns [`Error::FailedToGetUserFilePath`] for an unusable name.
pub fn get_user_file_path(user_name: &str) -> Result<PathOwned> {
    if user_name.is_empty()
        || matches!(user_name, "." | "..")
        || user_name
            .chars()
            .any(|character| character == '/' || character.is_control())
    {
        return Err(Error::FailedToGetUserFilePath);
    }
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
/// # Errors
///
/// - `FailedToGetUserFilePath` - Invalid username or path construction failure
/// - `FailedToOpenUserFile` - User file doesn't exist or permission denied
/// - `FailedToReadUserFile` - I/O error reading user file
/// - `FailedToParseUserFile` - Invalid JSON format in user file
/// - `InvalidPassword` - Password doesn't match stored hash
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

    let user = parse_user_record(&buffer, user_name)?;

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
/// The account records `/users/<user_name>` as its home. The home directory
/// itself is created lazily, at the first login, by
/// `VirtualFileSystem::ensure_user_home`.
///
/// # Errors
///
/// - `InvalidUserName` - The name does not satisfy [`users::is_valid_user_name`]
/// - `UserHomeAlreadyUsed` - Another account records that home (for example a
///   renamed account that kept it), or it exists and is not a directory owned
///   by the new account
/// - `FailedToCreateUser(DuplicateUserName)` - An account record with that
///   name already exists, even one this firmware could not load
/// - User identifier generation or assignment failures
/// - File system operations (directory creation, file writing)
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

    let home = new_user_home(user_name)?;

    // - New user identifier if not provided.
    let user_identifier = if let Some(user_identifier) = user_identifier {
        user_identifier
    } else {
        users_manager
            .get_new_user_identifier()
            .await
            .map_err(Error::FailedToGetNewUserIdentifier)?
    };

    // - Never hand an existing home to a different account.
    if users_manager.is_user_home(&home).await {
        return Err(Error::UserHomeAlreadyUsed);
    }
    match virtual_file_system.get_statistics(&home.as_str()).await {
        Ok(statistics)
            if statistics.user != user_identifier || statistics.kind != Kind::Directory =>
        {
            return Err(Error::UserHomeAlreadyUsed);
        }
        Ok(_) | Err(file_system::IoErrorKind::NotFound) => {}
        Err(error) => return Err(Error::FailedToReadUserFile(error)),
    }

    // - Never overwrite an existing record, including one that was skipped
    //   at load time (unsupported version, invalid home).
    let user_file_path = get_user_file_path(user_name)?;
    match virtual_file_system.get_statistics(&user_file_path).await {
        Ok(_) => {
            return Err(Error::FailedToCreateUser(users::Error::DuplicateUserName));
        }
        Err(file_system::IoErrorKind::NotFound) => {}
        Err(error) => return Err(Error::FailedToReadUserFile(error)),
    }

    // - Add it to the users manager.
    users_manager
        .add_user(user_identifier, user_name, primary_group, &home)
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
        home,
        hash,
        salt,
    );

    match Directory::create(virtual_file_system, task, USERS_FOLDER_PATH).await {
        Ok(_) | Err(virtual_file_system::IoErrorKind::AlreadyExists) => {}
        Err(error) => Err(Error::FailedToCreateUsersDirectory(error))?,
    }

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
/// The record is rewritten in the current format: a version 0 record is
/// upgraded to version 1 and records the home it already used
/// (`/users/<identifier>`), so its home does not move.
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

    let user_file_path = get_user_file_path(user_name)?;

    let mut buffer = Vec::new();

    File::read_from_path(virtual_file_system, task, &user_file_path, &mut buffer)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    let mut user = parse_user_record(&buffer, user_name)?;

    user.set_hash(hash);
    user.set_salt(salt);

    // Replaced atomically: a power loss leaves the old or the new password,
    // never a truncated record.
    atomic_write(
        context,
        &temporary_record_path(user.get_identifier()),
        user_file_path.as_str(),
        miniserde::json::to_string(&user).as_bytes(),
    )
    .await
}

/// Renames a user account.
///
/// Like Linux `usermod --login` without `--move-home`, renaming keeps the
/// recorded home: `alice` renamed to `alicia` keeps `/users/alice`, and its
/// contents are neither moved nor lost. Because that home stays recorded, the
/// name `alice` cannot be given to a new account: `create_user` refuses it
/// with `UserHomeAlreadyUsed`.
///
/// The record file is moved to its new name first, which commits the rename,
/// and the registry is updated right away. The record's contents are then
/// replaced atomically with the new name (and, for a version 0 record,
/// upgraded to version 1 with its `/users/<identifier>` home). That rewrite is
/// best effort: the record's file name is authoritative, so if it fails, or
/// power is lost before it, the account still loads under its new name. A
/// failed rewrite is logged and the rename still succeeds.
///
/// # Errors
///
/// - `InvalidUserName` - The new name does not satisfy [`users::is_valid_user_name`]
/// - `FailedToRenameUser` - The new name is already used, or the account is
///   not registered
/// - File system errors (reading, moving, writing user file)
/// - JSON parsing errors
pub async fn change_user_name(
    context: &Context<'_>,
    current_name: &str,
    new_name: &str,
) -> Result<()> {
    if !users::is_valid_user_name(new_name) {
        return Err(Error::InvalidUserName);
    }
    if current_name == new_name {
        return Ok(());
    }

    let virtual_file_system = context.virtual_file_system;
    let task = context.task;
    let users_manager = context.users_manager;
    let current_path = get_user_file_path(current_name)?;
    let new_path = get_user_file_path(new_name)?;

    let mut buffer = Vec::new();

    File::read_from_path(virtual_file_system, task, &current_path, &mut buffer)
        .await
        .map_err(Error::FailedToReadUserFile)?;

    let mut user = parse_user_record(&buffer, current_name)?;
    let identifier = user.get_identifier();

    if users_manager.get_user_name(identifier).await.as_deref() != Ok(current_name) {
        return Err(Error::FailedToRenameUser(
            users::Error::InvalidUserIdentifier,
        ));
    }
    if users_manager.get_user_identifier(new_name).await.is_ok() {
        return Err(Error::FailedToRenameUser(users::Error::DuplicateUserName));
    }
    match virtual_file_system.get_statistics(&new_path).await {
        Ok(_) => {
            return Err(Error::FailedToRenameUser(users::Error::DuplicateUserName));
        }
        Err(file_system::IoErrorKind::NotFound) => {}
        Err(error) => return Err(Error::FailedToReadUserFile(error)),
    }

    virtual_file_system
        .rename(task, &current_path, &new_path)
        .await
        .map_err(Error::FailedToWriteUserFile)?;

    // The rename is committed on disk: the registry must follow at once, so a
    // failed rewrite below cannot leave the new name looking free.
    users_manager
        .rename_user(identifier, new_name)
        .await
        .map_err(Error::FailedToRenameUser)?;

    user.set_name(new_name.to_string());

    if let Err(error) = atomic_write(
        context,
        &temporary_record_path(identifier),
        new_path.as_str(),
        miniserde::json::to_string(&user).as_bytes(),
    )
    .await
    {
        log::warning!(
            "Account {} renamed, but its record could not be rewritten: {error}",
            identifier.as_u16()
        );
    }

    Ok(())
}

/// Reads and parses a user file from the filesystem.
///
/// Version 0 records get their `/users/<identifier>` home in memory; nothing
/// is written back.
///
/// # Errors
///
/// - Path construction failures
/// - File system errors (opening, reading)
/// - JSON parsing errors, invalid recorded homes and unsupported versions
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

    parse_user_record(buffer, file)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    fn parse(json: &str) -> Result<User> {
        parse_user_record(json.as_bytes(), "bob")
    }

    #[test]
    fn version_zero_records_fall_back_to_the_identifier_home() {
        let user =
            parse(r#"{"identifier":42,"name":"bob","primary_group":1000,"hash":"h","salt":"s"}"#)
                .unwrap();
        assert_eq!(user.get_home(), "/users/42");
        assert_eq!(user.get_name(), "bob");
        // Rewriting a version 0 record records the home it already used.
        let written = miniserde::json::to_string(&user);
        assert!(written.contains(r#""version":1"#), "{written}");
        assert!(written.contains(r#""home":"/users/42""#), "{written}");
        assert_eq!(parse(&written).unwrap().get_home(), "/users/42");
    }

    #[test]
    fn version_one_records_keep_their_recorded_home() {
        let user = parse(
            r#"{"version":1,"identifier":42,"name":"bob","primary_group":1000,"home":"/users/robert","hash":"h","salt":"s"}"#,
        )
        .unwrap();
        assert_eq!(user.get_home(), "/users/robert");
    }

    #[test]
    fn malformed_or_future_records_are_rejected() {
        for (json, expected) in [
            (
                r#"{"version":1,"identifier":42,"name":"bob","primary_group":1000,"hash":"h","salt":"s"}"#,
                "home",
            ),
            (
                r#"{"version":1,"identifier":42,"name":"bob","primary_group":1000,"home":"/system","hash":"h","salt":"s"}"#,
                "home",
            ),
            (
                r#"{"version":1,"identifier":42,"name":"bob","primary_group":1000,"home":"/users/../system","hash":"h","salt":"s"}"#,
                "home",
            ),
            (
                r#"{"identifier":42,"name":"bob","primary_group":1000,"home":"/users/bob","hash":"h","salt":"s"}"#,
                "home",
            ),
            (
                r#"{"version":2,"identifier":42,"name":"bob","primary_group":1000,"home":"/users/bob","hash":"h","salt":"s"}"#,
                "version",
            ),
        ] {
            match (parse(json), expected) {
                (Err(Error::InvalidUserHome), "home")
                | (Err(Error::UnsupportedUserRecordVersion), "version") => {}
                (result, _) => panic!("{json}: {result:?}"),
            }
        }
    }

    #[test]
    fn the_record_file_name_is_authoritative() {
        let user = parse_user_record(
            br#"{"version":1,"identifier":42,"name":"alice","primary_group":1000,"home":"/users/alice","hash":"h","salt":"s"}"#,
            "alicia",
        )
        .unwrap();
        assert_eq!(user.get_name(), "alicia");
        assert_eq!(user.get_home(), "/users/alice");
    }

    #[test]
    fn new_homes_are_named_after_valid_user_names() {
        assert_eq!(new_user_home("alice").unwrap(), "/users/alice");
        for name in ["", ".", "..", "a/b", "../root", "a\0", "1000", "with space"] {
            assert!(
                matches!(new_user_home(name), Err(Error::InvalidUserName)),
                "{name:?}"
            );
        }
    }

    #[test]
    fn record_paths_stay_inside_the_accounts_directory() {
        assert_eq!(
            get_user_file_path("alice").unwrap().as_str(),
            "/system/accounts/users/alice"
        );
        for name in ["", ".", "..", "../groups/administrator", "a/b", "a\0"] {
            assert!(get_user_file_path(name).is_err(), "{name:?}");
        }
    }
}
