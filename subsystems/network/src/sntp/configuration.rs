//! The SNTP service configuration: `/configuration/system/sntp.json`, with the
//! defaults used when the file is absent.

use core::time::Duration;

use alloc::string::String;
use file_system::{AccessFlags, IoErrorKind, Kind};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use miniserde::Deserialize;
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

/// The configuration file of the SNTP service. Only root may write it, like the
/// other files of `/configuration/system`.
pub const CONFIGURATION_PATH: &str = "/configuration/system/sntp.json";

/// The largest configuration file read, in bytes.
pub const MAXIMUM_CONFIGURATION_SIZE: usize = 1024;

/// The largest number of configured servers.
pub const MAXIMUM_SERVERS: usize = 4;

/// The largest server name kept, in bytes (the DNS limit is 253).
pub const MAXIMUM_SERVER_NAME_LENGTH: usize = 253;

/// The server queried when the configuration file names none.
pub const DEFAULT_SERVER: &str = "pool.ntp.org";

/// How often UTC is resynchronized after a success, when the configuration
/// file says nothing.
pub const DEFAULT_RESYNC_INTERVAL: Duration = Duration::from_secs(3600);

/// One server name, by host name or IP address.
pub type ServerName = HeaplessString<MAXIMUM_SERVER_NAME_LENGTH>;

/// The configured server list.
pub type ServerList = HeaplessVec<ServerName, MAXIMUM_SERVERS>;

/// The parsed SNTP configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    /// The servers queried in order until one answers.
    pub servers: ServerList,
    /// How long to wait after a success before resynchronizing.
    pub resync_interval: Duration,
}

impl Default for Configuration {
    fn default() -> Self {
        let mut servers = ServerList::new();
        servers
            .push(ServerName::try_from(DEFAULT_SERVER).expect("the default server name fits"))
            .expect("one server fits");
        Self {
            servers,
            resync_interval: DEFAULT_RESYNC_INTERVAL,
        }
    }
}

/// The file as written. Unknown keys, such as `$schema`, are ignored.
#[derive(Deserialize)]
struct ConfigurationFile {
    servers: Option<alloc::vec::Vec<String>>,
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
    /// A server name longer than the DNS limit.
    ServerNameTooLong,
    /// A zero resynchronization interval.
    InvalidInterval,
}

/// Parse a configuration file body.
pub fn parse(text: &str) -> Result<Configuration, ConfigurationError> {
    let file: ConfigurationFile =
        miniserde::json::from_str(text).map_err(|_| ConfigurationError::Syntax)?;

    let servers = match file.servers {
        Some(names) if names.is_empty() => return Err(ConfigurationError::EmptyServerList),
        Some(names) => {
            let mut servers = ServerList::new();
            for name in names {
                let name = ServerName::try_from(name.as_str())
                    .map_err(|_| ConfigurationError::ServerNameTooLong)?;
                servers
                    .push(name)
                    .map_err(|_| ConfigurationError::TooManyServers)?;
            }
            servers
        }
        None => Configuration::default().servers,
    };

    let resync_interval = match file.resync_interval_seconds {
        Some(0) => return Err(ConfigurationError::InvalidInterval),
        Some(seconds) => Duration::from_secs(seconds),
        None => DEFAULT_RESYNC_INTERVAL,
    };

    Ok(Configuration {
        servers,
        resync_interval,
    })
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
        if statistics.kind != Kind::File
            || statistics.user.as_u16() != 0
            || !statistics.permissions.get_group().is_empty()
            || !statistics.permissions.get_others().is_empty()
        {
            return Err(IoErrorKind::PermissionDenied);
        }
        let mut contents = [0u8; MAXIMUM_CONFIGURATION_SIZE + 1];
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

    #[test]
    fn defaults_query_pool_ntp_org_hourly() {
        let configuration = Configuration::default();
        assert_eq!(configuration.servers.len(), 1);
        assert_eq!(configuration.servers[0].as_str(), "pool.ntp.org");
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
            r#"{"servers": ["time.example.com", "192.0.2.1"], "resync_interval_seconds": 60}"#,
        )
        .unwrap();
        assert_eq!(configuration.servers.len(), 2);
        assert_eq!(configuration.servers[0].as_str(), "time.example.com");
        assert_eq!(configuration.servers[1].as_str(), "192.0.2.1");
        assert_eq!(configuration.resync_interval, Duration::from_secs(60));
    }

    #[test]
    fn invalid_files_are_rejected() {
        assert_eq!(parse("not json"), Err(ConfigurationError::Syntax));
        assert_eq!(
            parse(r#"{"servers": []}"#),
            Err(ConfigurationError::EmptyServerList)
        );
        assert_eq!(
            parse(r#"{"servers": ["a", "b", "c", "d", "e"]}"#),
            Err(ConfigurationError::TooManyServers)
        );
        assert_eq!(
            parse(&format!(r#"{{"servers": ["{}"]}}"#, "a".repeat(254))),
            Err(ConfigurationError::ServerNameTooLong)
        );
        assert_eq!(
            parse(r#"{"resync_interval_seconds": 0}"#),
            Err(ConfigurationError::InvalidInterval)
        );
        assert_eq!(
            parse(r#"{"servers": "pool.ntp.org"}"#),
            Err(ConfigurationError::Syntax)
        );
    }

    #[test]
    fn a_full_server_list_is_accepted() {
        let json = r#"{"servers": ["a", "b", "c", "d"]}"#;
        assert_eq!(parse(json).unwrap().servers.len(), MAXIMUM_SERVERS);
    }
}
