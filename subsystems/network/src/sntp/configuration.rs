//! The SNTP service configuration: `/configuration/system/sntp.json`, with the
//! defaults used when the file is absent.
//!
//! The file is a JSON object with two optional keys:
//!
//! ```json
//! {
//!   "servers": ["time.example.com", "192.0.2.1"],
//!   "resync_interval_seconds": 3600
//! }
//! ```
//!
//! `servers` is a list of at most [`MAXIMUM_SERVERS`] host names or addresses,
//! each at most [`MAXIMUM_SERVER_NAME_LENGTH`] bytes (the DNS limit); the
//! default is `["pool.ntp.org"]`, a placeholder only (see [`DEFAULT_SERVER`]).
//! `resync_interval_seconds` is clamped to [`MINIMUM_RESYNC_INTERVAL`] up to
//! [`MAXIMUM_RESYNC_INTERVAL`]; the default is one hour. Unknown keys, such as
//! `$schema`, are ignored.

use core::time::Duration;

use alloc::{string::String, string::ToString, vec, vec::Vec};
use file_system::{AccessFlags, IoErrorKind, Kind, Permission, Statistics};
use miniserde::Deserialize;
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, VirtualFileSystem};

/// The configuration file of the SNTP service. Only root may have written it,
/// like the other files of `/configuration/system`.
pub const CONFIGURATION_PATH: &str = "/configuration/system/sntp.json";

/// The largest configuration file read, in bytes.
pub const MAXIMUM_CONFIGURATION_SIZE: usize = 1024;

/// The largest number of configured servers.
pub const MAXIMUM_SERVERS: usize = 3;

/// The largest server name kept, in bytes: the DNS limit for a host name.
pub const MAXIMUM_SERVER_NAME_LENGTH: usize = 253;

/// The server queried when the configuration file names none.
///
/// This is a placeholder only. The NTP Pool asks vendors not to use the
/// standard `pool.ntp.org` names as a default configuration in a shipped
/// product: "You must absolutely not use the default pool.ntp.org zone names as
/// the default configuration in your application or appliance." A vendor zone
/// such as `xila.pool.ntp.org` must be requested from the NTP Pool by the
/// project owner before release; see <https://www.ntppool.org/vendors.html>.
pub const DEFAULT_SERVER: &str = "pool.ntp.org";

/// How often UTC is resynchronized after a success, when the configuration
/// file says nothing.
pub const DEFAULT_RESYNC_INTERVAL: Duration = Duration::from_secs(3600);

/// The smallest accepted resynchronization interval.
pub const MINIMUM_RESYNC_INTERVAL: Duration = Duration::from_secs(60);

/// The largest accepted resynchronization interval: 7 days.
pub const MAXIMUM_RESYNC_INTERVAL: Duration = Duration::from_secs(7 * 24 * 3600);

/// The parsed SNTP configuration. The server names are heap allocated once at
/// startup, so the struct itself stays small.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    /// The servers queried in order until one answers.
    pub servers: Vec<String>,
    /// How long to wait after a success before resynchronizing.
    pub resync_interval: Duration,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            servers: vec![DEFAULT_SERVER.to_string()],
            resync_interval: DEFAULT_RESYNC_INTERVAL,
        }
    }
}

/// The file as written. Unknown keys, such as `$schema`, are ignored.
#[derive(Deserialize)]
struct ConfigurationFile {
    servers: Option<Vec<String>>,
    resync_interval_seconds: Option<u64>,
}

/// Why a configuration file was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigurationError {
    /// Not a JSON object with the expected value types.
    Syntax,
    /// An empty server list.
    EmptyServerList,
    /// More servers than the service keeps.
    TooManyServers,
    /// A server name longer than the DNS limit, or empty.
    InvalidServerName,
}

/// Parse a configuration file body.
pub fn parse(text: &str) -> Result<Configuration, ConfigurationError> {
    let file: ConfigurationFile =
        miniserde::json::from_str(text).map_err(|_| ConfigurationError::Syntax)?;

    let servers = match file.servers {
        Some(names) if names.is_empty() => return Err(ConfigurationError::EmptyServerList),
        Some(names) => {
            if names.len() > MAXIMUM_SERVERS {
                return Err(ConfigurationError::TooManyServers);
            }
            for name in &names {
                if name.is_empty() || name.len() > MAXIMUM_SERVER_NAME_LENGTH {
                    return Err(ConfigurationError::InvalidServerName);
                }
            }
            names
        }
        None => Configuration::default().servers,
    };

    let resync_interval = match file.resync_interval_seconds {
        Some(seconds) => {
            Duration::from_secs(seconds).clamp(MINIMUM_RESYNC_INTERVAL, MAXIMUM_RESYNC_INTERVAL)
        }
        None => DEFAULT_RESYNC_INTERVAL,
    };

    Ok(Configuration {
        servers,
        resync_interval,
    })
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

/// Load the configuration, falling back to the defaults when the file is
/// absent, unreadable or invalid (a warning says so once, at startup).
pub async fn load(virtual_file_system: &VirtualFileSystem, task: TaskIdentifier) -> Configuration {
    let mut file = match File::open(
        virtual_file_system,
        task,
        CONFIGURATION_PATH,
        AccessFlags::Read.into(),
    )
    .await
    {
        Ok(file) => file,
        Err(IoErrorKind::NotFound) => return Configuration::default(),
        Err(error) => {
            log::warning!("SNTP: cannot read {CONFIGURATION_PATH} ({error}); using the defaults");
            return Configuration::default();
        }
    };

    let result = async {
        let statistics = file.get_statistics().await?;
        if statistics.kind != Kind::File || !is_written_by_root_only(&statistics) {
            return Err(IoErrorKind::PermissionDenied);
        }
        // The buffer is allocated once, on the heap, outside the task stack.
        let mut contents = vec![0u8; MAXIMUM_CONFIGURATION_SIZE + 1];
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
        parse(text).map_err(|_| IoErrorKind::InvalidData)
    }
    .await;

    let closed = file.close(virtual_file_system).await;
    let outcome = result.and_then(|configuration| closed.map(|()| configuration));
    match outcome {
        Ok(configuration) => configuration,
        Err(error) => {
            log::warning!("SNTP: {CONFIGURATION_PATH} rejected ({error}); using the defaults");
            Configuration::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use file_system::{Kind, Permissions, Statistics, Time};
    use users::{GroupIdentifier, UserIdentifier};

    fn statistics(user: u16, group: u16, permissions: u16) -> Statistics {
        Statistics::new(
            0,
            1,
            10,
            Time::new(0),
            Time::new(0),
            Time::new(0),
            Time::new(0),
            Kind::File,
            Permissions::from_octal(permissions).unwrap(),
            UserIdentifier::new(user),
            GroupIdentifier::new(group),
        )
    }

    #[test]
    fn only_root_may_have_written_the_configuration_file() {
        assert!(is_written_by_root_only(&statistics(0, 0, 0o644)));
        assert!(is_written_by_root_only(&statistics(0, 0, 0o600)));
        // The group may write only when it is root's group.
        assert!(is_written_by_root_only(&statistics(0, 0, 0o664)));
        assert!(!is_written_by_root_only(&statistics(0, 0, 0o646)));
        assert!(!is_written_by_root_only(&statistics(0, 1000, 0o664)));
        // Not owned by root.
        assert!(!is_written_by_root_only(&statistics(1000, 0, 0o644)));
    }

    #[test]
    fn defaults_query_pool_ntp_org_hourly() {
        let configuration = Configuration::default();
        assert_eq!(configuration.servers, ["pool.ntp.org".to_string()]);
        assert_eq!(configuration.resync_interval, Duration::from_secs(3600));
    }

    #[test]
    fn empty_object_gives_the_defaults() {
        assert_eq!(parse("{}").unwrap(), Configuration::default());
        assert_eq!(
            parse(r#"{"$schema": "https://example.invalid/sntp.schema.json"}"#).unwrap(),
            Configuration::default()
        );
    }

    #[test]
    fn servers_and_interval_are_read() {
        let configuration = parse(
            r#"{"servers": ["time.example.com", "192.0.2.1"], "resync_interval_seconds": 600}"#,
        )
        .unwrap();
        assert_eq!(
            configuration.servers,
            ["time.example.com".to_string(), "192.0.2.1".to_string()]
        );
        assert_eq!(configuration.resync_interval, Duration::from_secs(600));
    }

    #[test]
    fn the_interval_is_clamped_to_the_bounds() {
        assert_eq!(
            parse(r#"{"resync_interval_seconds": 0}"#)
                .unwrap()
                .resync_interval,
            MINIMUM_RESYNC_INTERVAL
        );
        assert_eq!(
            parse(r#"{"resync_interval_seconds": 1}"#)
                .unwrap()
                .resync_interval,
            MINIMUM_RESYNC_INTERVAL
        );
        assert_eq!(
            parse(r#"{"resync_interval_seconds": 999999999}"#)
                .unwrap()
                .resync_interval,
            MAXIMUM_RESYNC_INTERVAL
        );
        assert_eq!(
            parse(r#"{"resync_interval_seconds": 604800}"#)
                .unwrap()
                .resync_interval,
            MAXIMUM_RESYNC_INTERVAL
        );
    }

    #[test]
    fn invalid_files_are_rejected() {
        assert_eq!(parse("not json"), Err(ConfigurationError::Syntax));
        assert_eq!(
            parse(r#"{"servers": []}"#),
            Err(ConfigurationError::EmptyServerList)
        );
        assert_eq!(
            parse(r#"{"servers": ["a", "b", "c", "d"]}"#),
            Err(ConfigurationError::TooManyServers)
        );
        assert_eq!(
            parse(r#"{"servers": [""]}"#),
            Err(ConfigurationError::InvalidServerName)
        );
        assert_eq!(
            parse(&format!(
                r#"{{"servers": ["{}"]}}"#,
                "a".repeat(MAXIMUM_SERVER_NAME_LENGTH + 1)
            )),
            Err(ConfigurationError::InvalidServerName)
        );
        assert_eq!(
            parse(r#"{"servers": "pool.ntp.org"}"#),
            Err(ConfigurationError::Syntax)
        );
    }

    #[test]
    fn a_full_server_list_is_accepted() {
        let json = r#"{"servers": ["a", "b", "c"]}"#;
        assert_eq!(parse(json).unwrap().servers.len(), MAXIMUM_SERVERS);
        let json = format!(
            r#"{{"servers": ["{}"]}}"#,
            "a".repeat(MAXIMUM_SERVER_NAME_LENGTH)
        );
        assert_eq!(parse(&json).unwrap().servers.len(), 1);
    }
}
