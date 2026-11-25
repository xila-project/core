//! The system time zone setting of Xila.
//!
//! Civil time stays outside the kernel clock: the `time` subsystem keeps UTC
//! and monotonic time only (xila-project/core#463). This crate stores the time
//! zone that the user chose, loads it at start-up and publishes it to every
//! task as the POSIX `TZ` environment variable. Programs convert UTC to local
//! time with the `xila-time-zone` crate.
//!
//! # Stored setting
//!
//! [`CONFIGURATION_PATH`] (`/configuration/system/time_zone.json`) holds one
//! JSON object:
//!
//! ```json
//! { "name": "Europe/Paris", "time_zone": "CET-1CEST,M3.5.0,M10.5.0/3" }
//! ```
//!
//! - `time_zone` (required) is the POSIX TZ string, the `TZ` format of POSIX
//!   `tzset`. It is the only value that Core reads and publishes, so Core needs
//!   no IANA zone table.
//! - `name` (optional) is the IANA zone name that the user chose, for display
//!   only. It is absent when the user entered a custom TZ string. A program
//!   with a newer zone table can resolve the name again. An invalid name is
//!   dropped and the TZ string still applies.
//!
//! Only root may have written the file: it belongs to root, others cannot
//! write it, and its group can only if it is root's group, as for the other
//! files of `/configuration/system`. Everyone can read it. [`set`] replaces it
//! atomically (a temporary file created with mode 0644, then a rename; a
//! temporary file left by a failed write is truncated by the next one). A
//! missing, unreadable or invalid file gives UTC (`UTC0`) and one warning at
//! start-up, from [`initialize`]; [`load`] logs nothing. It never blocks the
//! start-up.
//!
//! # Publication
//!
//! [`initialize`] runs once at start-up, before the shell, and sets `TZ` to the
//! stored TZ string in the root task and in every task that has no `TZ` yet.
//! Every task inherits the environment of its parent, so native executables
//! and WebAssembly guests (through WASI `environ_get`) see `TZ` from their
//! start.
//!
//! [`set`] writes the file and then gives `TZ` the new value in the root task
//! and in every task whose `TZ` is still the previous system value
//! ([`task::Manager::set_system_environment_variable`]); a task that set a `TZ`
//! of its own keeps it, unless it set the system value itself: such a task
//! cannot be told apart from one that inherited the value, so it follows the
//! change. Calls of [`set`] are serialised, so the file and `TZ` always hold
//! the same setting. Tasks started after the change get the new value at
//! once. A native program that reads `TZ` again, for example through
//! [`current`] on each display update, follows the change. A WebAssembly guest
//! copies its environment when it starts, so a running guest keeps the old
//! value until it is restarted.
//!
//! # Sizes
//!
//! On 32-bit targets, [`Setting`] is 24 bytes (two heap strings, the name with
//! the niche of `Option`) and [`SettingError`] is 1 byte.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use core::cell::RefCell;
use file_system::{
    AccessFlags, CreateFlags, Flags, IntoIoErrorKind, IoErrorKind, Kind, Path, Permission,
    Permissions, Special, Statistics,
};
use synchronization::{
    blocking_mutex::{Mutex as BlockingMutex, raw::CriticalSectionRawMutex},
    mutex::Mutex,
};
use task::TaskIdentifier;
use users::{ADMINISTRATOR_GROUP_NAME, GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, VirtualFileSystem};

pub use time_zone::{self, ParseError, TimeZone};

/// The file that holds the setting.
pub const CONFIGURATION_PATH: &str = "/configuration/system/time_zone.json";

/// The temporary file that [`set`] writes before it renames it.
const TEMPORARY_PATH: &str = "/configuration/system/.time_zone.json-new";

/// The environment variable that holds the POSIX TZ string of the system
/// time zone.
pub const ENVIRONMENT_VARIABLE: &str = "TZ";

/// The TZ string used when no valid setting is stored: UTC.
pub const DEFAULT_TIME_ZONE: &str = "UTC0";

/// The longest TZ string accepted, in bytes. The longest TZ string of tzdata
/// 2026f has 50 bytes (`Pacific/Chatham`).
pub const MAXIMUM_TIME_ZONE_LENGTH: usize = 64;

/// The longest zone name accepted, in bytes. The longest IANA name of tzdata
/// 2026f has 32 bytes (`America/Argentina/ComodRivadavia`).
pub const MAXIMUM_NAME_LENGTH: usize = 64;

/// The largest configuration file read, in bytes.
pub const MAXIMUM_CONFIGURATION_SIZE: usize = 256;

/// Why a setting is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingError {
    /// The name is empty, too long, or has a character that IANA names do not
    /// use (only ASCII letters, digits, `/`, `_`, `-` and `+` are accepted).
    InvalidName,
    /// The TZ string is longer than [`MAXIMUM_TIME_ZONE_LENGTH`].
    TimeZoneTooLong,
    /// The TZ string does not parse.
    InvalidTimeZone(ParseError),
}

impl core::fmt::Display for SettingError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SettingError::InvalidName => write!(formatter, "invalid time zone name"),
            SettingError::TimeZoneTooLong => write!(formatter, "TZ string too long"),
            SettingError::InvalidTimeZone(error) => {
                write!(formatter, "invalid TZ string: {error:?}")
            }
        }
    }
}

/// A valid time zone setting: an optional zone name and a POSIX TZ string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    name: Option<String>,
    time_zone: String,
}

impl Default for Setting {
    fn default() -> Self {
        Self::utc()
    }
}

impl Setting {
    /// UTC, with no name.
    pub fn utc() -> Self {
        Self {
            name: None,
            time_zone: DEFAULT_TIME_ZONE.to_string(),
        }
    }

    /// Check and build a setting. `name` is the IANA zone name that
    /// `time_zone` was resolved from, or None for a custom TZ string. The name
    /// is not looked up: Core has no zone table.
    pub fn new(name: Option<&str>, time_zone: &str) -> Result<Self, SettingError> {
        if let Some(name) = name
            && !is_valid_name(name)
        {
            return Err(SettingError::InvalidName);
        }
        validate_time_zone(time_zone)?;
        Ok(Self {
            name: name.map(ToString::to_string),
            time_zone: time_zone.to_string(),
        })
    }

    /// The IANA zone name, or None for a custom TZ string.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The POSIX TZ string.
    pub fn time_zone_string(&self) -> &str {
        &self.time_zone
    }

    /// The parsed time zone.
    pub fn time_zone(&self) -> TimeZone {
        // The string was checked when the setting was built.
        TimeZone::parse(&self.time_zone).unwrap_or_else(|_| utc())
    }

    /// Parse the JSON body of a configuration file. An invalid `name` is
    /// dropped: it is for display only, and the TZ string still applies.
    pub fn from_json(text: &str) -> Option<Self> {
        let file: ConfigurationFile = miniserde::json::from_str(text).ok()?;
        let name = file.name.as_deref().filter(|name| is_valid_name(name));
        Self::new(name, &file.time_zone).ok()
    }

    /// The JSON body of a configuration file.
    pub fn to_json(&self) -> String {
        let mut text = miniserde::json::to_string(&ConfigurationFile {
            name: self.name.clone(),
            time_zone: self.time_zone.clone(),
        });
        text.push('\n');
        text
    }
}

/// The file as written. Unknown keys are ignored.
#[derive(miniserde::Serialize, miniserde::Deserialize)]
struct ConfigurationFile {
    name: Option<String>,
    time_zone: String,
}

/// Check a POSIX TZ string as a setting: its length and its syntax.
pub fn validate_time_zone(time_zone: &str) -> Result<TimeZone, SettingError> {
    if time_zone.len() > MAXIMUM_TIME_ZONE_LENGTH {
        return Err(SettingError::TimeZoneTooLong);
    }
    TimeZone::parse(time_zone).map_err(SettingError::InvalidTimeZone)
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAXIMUM_NAME_LENGTH
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'+'))
}

/// UTC as a parsed time zone.
pub fn utc() -> TimeZone {
    // `UTC0` always parses; the fallback keeps the function total.
    TimeZone::parse(DEFAULT_TIME_ZONE).expect("UTC0 is a valid TZ string")
}

/// Whether only root can have written a file: it belongs to root, others may
/// not write it, and its group may only if it is root's group.
fn is_written_by_root_only(statistics: &Statistics) -> bool {
    statistics.user == UserIdentifier::ROOT
        && !statistics
            .permissions
            .get_others()
            .contains(Permission::Write)
        && (statistics.group == GroupIdentifier::ROOT
            || !statistics
                .permissions
                .get_group()
                .contains(Permission::Write))
}

/// Read the stored setting, without a fallback.
///
/// Fails with `NotFound` when there is no file, `PermissionDenied` when the
/// file is not a regular file written by root only, `FileTooLarge` when it is
/// larger than [`MAXIMUM_CONFIGURATION_SIZE`], and `InvalidData` when its
/// contents are not a valid setting.
pub async fn read(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<Setting, IoErrorKind> {
    let mut file = File::open(
        virtual_file_system,
        task,
        CONFIGURATION_PATH,
        AccessFlags::Read.into(),
    )
    .await?;

    let result = async {
        let statistics = file.get_statistics().await?;
        if statistics.kind != Kind::File || !is_written_by_root_only(&statistics) {
            return Err(IoErrorKind::PermissionDenied);
        }
        // One byte more than the limit tells a full file from a larger one.
        // The buffer is on the heap for the duration of the read, so it does
        // not stay in the state of the caller's future.
        let mut contents = alloc::vec![0u8; MAXIMUM_CONFIGURATION_SIZE + 1];
        let mut length = 0;
        while length < contents.len() {
            let received = file.read(&mut contents[length..]).await?;
            if received == 0 {
                break;
            }
            length += received;
        }
        if length > MAXIMUM_CONFIGURATION_SIZE {
            return Err(IoErrorKind::FileTooLarge);
        }
        let text =
            core::str::from_utf8(&contents[..length]).map_err(|_| IoErrorKind::InvalidData)?;
        Setting::from_json(text).ok_or(IoErrorKind::InvalidData)
    }
    .await;

    let closed = file.close(virtual_file_system).await;
    result.and_then(|setting| closed.map(|()| setting))
}

/// Read the stored setting, or UTC when it is missing or invalid. It logs
/// nothing; [`initialize`] reports at start-up why UTC is used.
pub async fn load(virtual_file_system: &VirtualFileSystem, task: TaskIdentifier) -> Setting {
    read(virtual_file_system, task)
        .await
        .unwrap_or_else(|_| Setting::utc())
}

/// Load the stored setting and publish its TZ string as `TZ`, in the root task
/// and in every task without `TZ`. Call it once at start-up, after the file
/// systems are mounted and before the shell or the startup application starts.
/// It returns the setting in use, UTC when the stored one is missing or
/// invalid.
pub async fn initialize(
    task_manager: &task::Manager,
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Setting {
    let setting = match read(virtual_file_system, task).await {
        Ok(setting) => setting,
        Err(error) => {
            log::warning!("Time zone: {CONFIGURATION_PATH} not used ({error}); using UTC");
            Setting::utc()
        }
    };
    task_manager
        .set_system_environment_variable(ENVIRONMENT_VARIABLE, &setting.time_zone)
        .await;
    store(&setting);
    setting
}

/// The setting in use, as [`initialize`] loaded it or [`set`] stored it. It is
/// kept in RAM (24 bytes and two short heap strings), so that programs that
/// show the setting, such as Settings, do not read the file again.
static IN_USE: BlockingMutex<CriticalSectionRawMutex, RefCell<Option<Setting>>> =
    BlockingMutex::new(RefCell::new(None));

fn store(setting: &Setting) {
    IN_USE.lock(|in_use| *in_use.borrow_mut() = Some(setting.clone()));
}

/// The setting in use: the one that [`initialize`] loaded or that [`set`]
/// stored last, or UTC before [`initialize`]. It does not read the file.
pub fn stored() -> Setting {
    IN_USE.lock(|in_use| in_use.borrow().clone().unwrap_or_else(Setting::utc))
}

/// Serialises [`set`]: the file write and the publication of `TZ` of one call
/// finish before the next call starts, so the file and `TZ` always hold the
/// same setting. A blocking mutex is not usable here: the write awaits.
static SET_LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

/// Store a new setting and publish it.
///
/// The user of `task` must be root or a member of the administrator group;
/// otherwise the call fails with `PermissionDenied`. The file is written with
/// the identity of the root task, so it stays owned by root. The new TZ string
/// is published only once the file is in place. See the crate documentation
/// for which tasks see the change.
pub async fn set(
    task_manager: &task::Manager,
    users_manager: &users::Manager,
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    setting: &Setting,
) -> Result<(), IoErrorKind> {
    let user = task_manager
        .get_user(task)
        .await
        .map_err(IntoIoErrorKind::into_io_error_kind)?;
    if !is_administrator(users_manager, user).await {
        return Err(IoErrorKind::PermissionDenied);
    }

    let _guard = SET_LOCK.lock().await;
    write(
        virtual_file_system,
        task::Manager::ROOT_TASK_IDENTIFIER,
        setting.to_json().as_bytes(),
    )
    .await?;
    task_manager
        .set_system_environment_variable(ENVIRONMENT_VARIABLE, &setting.time_zone)
        .await;
    store(setting);
    Ok(())
}

/// Whether a user may change the system time zone: root, or a member of the
/// administrator group.
pub async fn is_administrator(users_manager: &users::Manager, user: UserIdentifier) -> bool {
    if user == UserIdentifier::ROOT {
        return true;
    }
    match users_manager
        .get_group_identifier(ADMINISTRATOR_GROUP_NAME)
        .await
    {
        Ok(group) => users_manager.is_in_group(user, group).await,
        Err(_) => false,
    }
}

/// Replace the configuration file atomically: write a temporary file, then
/// rename it over the old one. On failure the old file is unchanged.
async fn write(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    contents: &[u8],
) -> Result<(), IoErrorKind> {
    let temporary = Path::from_str(TEMPORARY_PATH);
    // Root can write; everyone can read, from the creation of the file on. A
    // temporary file left by an interrupted write is truncated and reused:
    // only root can create it, so its permissions are already these.
    let mut file = File::open_with_permissions(
        virtual_file_system,
        task,
        temporary,
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
        Permissions::new(
            Permission::READ_WRITE,
            Permission::Read,
            Permission::Read,
            Special::NONE,
        ),
    )
    .await?;
    let result = async {
        let mut written = 0;
        while written < contents.len() {
            let count = file.write(&contents[written..]).await?;
            if count == 0 {
                return Err(IoErrorKind::WriteZero);
            }
            written += count;
        }
        file.flush().await
    }
    .await;
    let closed = file.close(virtual_file_system).await;
    // A failed write leaves the temporary file; the next write truncates it.
    match result.and(closed) {
        Ok(()) => {
            virtual_file_system
                .rename(task, &temporary, &Path::from_str(CONFIGURATION_PATH))
                .await
        }
        Err(error) => Err(error),
    }
}

/// The time zone of a task: its `TZ` variable, parsed. UTC when the task has
/// no `TZ` or when it does not parse.
///
/// Native programs call it again when they update a display, so that they
/// follow a change made through [`set`]. It reads one variable and parses one
/// short string; it does not touch the file system.
pub async fn current(task_manager: &task::Manager, task: TaskIdentifier) -> TimeZone {
    match task_manager
        .get_environment_variable(task, ENVIRONMENT_VARIABLE)
        .await
    {
        Ok(variable) => TimeZone::parse(variable.get_value()).unwrap_or_else(|_| utc()),
        Err(_) => utc(),
    }
}

/// Write a UTC offset in seconds as `+HH:MM`, or `+HH:MM:SS` when the seconds
/// are not zero. For example, `3600` is `+01:00` and `-12600` is `-03:30`.
pub fn write_offset(output: &mut dyn core::fmt::Write, offset_seconds: i32) -> core::fmt::Result {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let magnitude = offset_seconds.unsigned_abs();
    let (hours, minutes, seconds) = (magnitude / 3600, magnitude / 60 % 60, magnitude % 60);
    if seconds == 0 {
        write!(output, "{sign}{hours:02}:{minutes:02}")
    } else {
        write!(output, "{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// Write the local time of `utc` (seconds since the Unix epoch) in `zone`,
/// for example `2026-03-29 03:00:00 CEST (UTC+02:00)`. Settings and the shell
/// share it, so it takes a `dyn` writer and is linked once.
pub fn write_local_time(
    output: &mut dyn core::fmt::Write,
    zone: &TimeZone,
    utc: i64,
) -> core::fmt::Result {
    let local = zone.from_utc(utc);
    let date_time = local.date_time;
    write!(
        output,
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {} (UTC",
        date_time.year,
        date_time.month,
        date_time.day,
        date_time.hour,
        date_time.minute,
        date_time.second,
        local.abbreviation,
    )?;
    write_offset(output, local.offset_seconds)?;
    output.write_char(')')
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::string::String;

    #[test]
    fn sizes_on_32_bit_targets() {
        if cfg!(target_pointer_width = "32") {
            assert_eq!(core::mem::size_of::<Setting>(), 24);
        }
        assert_eq!(core::mem::size_of::<SettingError>(), 1);
    }

    #[test]
    fn setting_validation() {
        let paris = Setting::new(Some("Europe/Paris"), "CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(paris.name(), Some("Europe/Paris"));
        assert_eq!(paris.time_zone().standard_abbreviation(), "CET");
        assert!(Setting::new(None, "<+0530>-5:30").is_ok());
        assert!(Setting::new(Some("Etc/GMT+5"), "<-05>5").is_ok());
        assert_eq!(
            Setting::new(Some(""), "UTC0"),
            Err(SettingError::InvalidName)
        );
        assert_eq!(
            Setting::new(Some("Europe/Paris\n"), "UTC0"),
            Err(SettingError::InvalidName)
        );
        assert_eq!(
            Setting::new(Some("Europe Paris"), "UTC0"),
            Err(SettingError::InvalidName)
        );
        assert!(matches!(
            Setting::new(None, "UTC"),
            Err(SettingError::InvalidTimeZone(_))
        ));
        assert!(matches!(
            Setting::new(None, "Europe/Paris"),
            Err(SettingError::InvalidTimeZone(_))
        ));
        let long = "A".repeat(MAXIMUM_TIME_ZONE_LENGTH + 1);
        assert_eq!(
            Setting::new(None, &long),
            Err(SettingError::TimeZoneTooLong)
        );
    }

    #[test]
    fn json_round_trip() {
        for setting in [
            Setting::new(Some("Europe/Paris"), "CET-1CEST,M3.5.0,M10.5.0/3").unwrap(),
            Setting::new(None, "<+0545>-5:45").unwrap(),
        ] {
            assert_eq!(Setting::from_json(&setting.to_json()), Some(setting));
        }
        assert_eq!(
            Setting::from_json(r#"{"time_zone":"UTC0","$schema":"x"}"#),
            Some(Setting::utc())
        );
        assert_eq!(Setting::from_json(r#"{"name":"Europe/Paris"}"#), None);
        assert_eq!(Setting::from_json(r#"{"time_zone":"bad"}"#), None);
        assert_eq!(Setting::from_json("UTC0"), None);
    }

    #[test]
    fn local_time_at_a_daylight_saving_change() {
        let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let mut output = String::new();
        write_local_time(&mut output, &paris, 1_774_745_999).unwrap();
        assert_eq!(output, "2026-03-29 01:59:59 CET (UTC+01:00)");
        output.clear();
        write_local_time(&mut output, &paris, 1_774_746_000).unwrap();
        assert_eq!(output, "2026-03-29 03:00:00 CEST (UTC+02:00)");
    }

    #[test]
    fn an_invalid_name_keeps_the_time_zone() {
        let setting = Setting::from_json(r#"{"name":"Not a name!","time_zone":"JST-9"}"#).unwrap();
        assert_eq!(setting.name(), None);
        assert_eq!(setting.time_zone_string(), "JST-9");
    }

    #[test]
    fn offsets() {
        let format = |offset| {
            let mut output = String::new();
            write_offset(&mut output, offset).unwrap();
            output
        };
        assert_eq!(format(0), "+00:00");
        assert_eq!(format(3600), "+01:00");
        assert_eq!(format(-12600), "-03:30");
        assert_eq!(format(20700), "+05:45");
        assert_eq!(format(-5), "-00:00:05");
    }
}
