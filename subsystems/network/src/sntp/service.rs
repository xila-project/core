//! The SNTP synchronization service: a task that starts when an interface has
//! an address, sets UTC from the configured servers through `sntpc`, and
//! resynchronizes periodically with a bounded exponential backoff on failure.
//!
//! Kiss-of-death answers (RFC 5905 section 7.4) get a per-server policy:
//! `DENY` and `RSTR` ban the server until the next configuration change or
//! reboot, and `RATE` doubles that server's resynchronization interval,
//! bounded. Each event is logged once per server.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use sntpc::{Error as SntpError, KissOfDeathCode};
use task::{Manager as TaskManager, TaskIdentifier};

use crate::{
    IntoIoErrorKind, Manager, Result,
    sntp::{
        client::{self, Applied, Client, Failure},
        configuration::{self, Configuration},
    },
};

/// How often to check that an interface has an address while waiting for the
/// network to come up.
pub const NETWORK_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The delay before the first retry after a failure.
pub const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(30);

/// The largest delay between two attempts after failures.
pub const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(1800);

/// After this many consecutive failures, and then again every
/// `FAILURE_LOG_STRIDE`-th failure, a warning is logged.
pub const REPEATED_FAILURE_LOG_THRESHOLD: u32 = 3;

/// The stride between two failure warnings once the threshold is reached.
pub const FAILURE_LOG_STRIDE: u32 = 10;

/// The largest multiplier a `RATE` kiss of death applies to one server's
/// resynchronization interval.
pub const MAXIMUM_RATE_MULTIPLIER: u32 = 64;

/// The delay before retrying after `failures` consecutive failures: the
/// initial delay doubled per failure, capped at the maximum.
pub fn retry_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(16);
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let millis = (INITIAL_RETRY_DELAY.as_millis() as u64)
        .saturating_mul(factor)
        .min(MAXIMUM_RETRY_DELAY.as_millis() as u64);
    Duration::from_millis(millis)
}

/// The per-server state the kiss-of-death policy keeps. It lives in memory
/// only: a reboot or a new configuration starts every server fresh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerState {
    /// `DENY` or `RSTR`: the server is not queried again.
    banned: bool,
    /// How many times a `RATE` kiss of death doubled this server's interval.
    rate_multiplier: u32,
    /// Whether the kiss of death was already logged for this server.
    kiss_logged: bool,
}

impl ServerState {
    /// The resynchronization interval of this server: the configured interval
    /// times the rate multiplier.
    fn interval(&self, base: Duration) -> Duration {
        base * self.rate_multiplier.max(1)
    }
}

/// Apply a kiss-of-death answer to one server's state. Returns the log line to
/// print when the event was not logged yet, `None` otherwise.
pub fn handle_kiss_of_death(state: &mut ServerState, code: KissOfDeathCode) -> Option<String> {
    let message = match code {
        KissOfDeathCode::Deny | KissOfDeathCode::Rstr => {
            state.banned = true;
            Some(alloc::format!(
                "denied access ({}); not used again until the next configuration change or reboot",
                code.as_str()
            ))
        }
        KissOfDeathCode::Rate => {
            state.rate_multiplier = state
                .rate_multiplier
                .max(1)
                .saturating_mul(2)
                .min(MAXIMUM_RATE_MULTIPLIER);
            Some(alloc::format!(
                "asked to slow down ({}); its resynchronization interval is now doubled",
                code.as_str()
            ))
        }
        _ => return None,
    };
    if state.kiss_logged {
        return None;
    }
    state.kiss_logged = true;
    message
}

/// One successful synchronization.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The server that answered.
    pub server: String,
    /// What applying the answer changed.
    pub applied: Applied,
    /// How long to wait before resynchronizing with this server.
    pub interval: Duration,
}

/// The result of one attempt over the configured servers.
#[derive(Clone, Debug)]
enum Attempt {
    Synchronized(Outcome),
    Failed(Failure),
    /// Every configured server denied access: nothing is queried until the
    /// next configuration change or reboot.
    AllBanned,
}

/// The SNTP service, assembled from the runtime subsystems it needs.
pub struct Service {
    task_manager: &'static TaskManager,
    network: &'static Manager,
    time: &'static time::Manager<'static>,
    virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
}

impl Service {
    pub fn new(
        task_manager: &'static TaskManager,
        network: &'static Manager,
        time: &'static time::Manager<'static>,
        virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
    ) -> Self {
        Self {
            task_manager,
            network,
            time,
            virtual_file_system,
        }
    }

    /// Load the configuration and spawn the service task. The task runs until
    /// the system stops; its identifier is returned.
    pub async fn start(self, task: TaskIdentifier) -> Result<TaskIdentifier> {
        // The configuration is allocated once, outside the task stack.
        let configuration: &'static Configuration = Box::leak(Box::new(
            configuration::load(self.virtual_file_system, task).await,
        ));
        let network = self.network;
        let time = self.time;
        self.task_manager
            .spawn(task, "SNTP synchronization", None, move |_| async move {
                run(network, time, configuration).await;
            })
            .await
            .map(|(_, identifier)| identifier)
            .map_err(|error| error.into_io_error_kind())
    }
}

/// The service loop: wait for the network, synchronize, resynchronize after the
/// configured interval, and back off exponentially (bounded) on failure.
async fn run(
    network: &'static Manager,
    time: &'static time::Manager<'static>,
    configuration: &'static Configuration,
) {
    let client = Client::new(network, time);
    let mut adapter: Option<client::SntpUdpSocket> = None;
    let mut states: Vec<ServerState> = configuration
        .servers
        .iter()
        .map(|_| ServerState::default())
        .collect();
    let mut failures: u32 = 0;
    let mut synchronized_once = false;

    loop {
        // Start when an interface has an address.
        while !network.has_ip_address(None).await {
            task::sleep(NETWORK_POLL_INTERVAL).await;
        }

        match attempt(&client, configuration, &mut states, &mut adapter).await {
            Attempt::Synchronized(outcome) => {
                if !synchronized_once {
                    synchronized_once = true;
                    log_success(&outcome);
                }
                failures = 0;
                task::sleep(outcome.interval).await;
            }
            Attempt::AllBanned => {
                // Nothing is queried until the next configuration change or
                // reboot; the kiss-of-death was already logged once.
                task::sleep(configuration.resync_interval).await;
            }
            Attempt::Failed(error) => {
                failures = failures.saturating_add(1);
                if let Failure::Io(kind) = error
                    && client::invalidates_socket(kind)
                {
                    // The interface may have changed: rebuild the socket.
                    adapter = None;
                }
                if failures >= REPEATED_FAILURE_LOG_THRESHOLD
                    && (failures - REPEATED_FAILURE_LOG_THRESHOLD)
                        .is_multiple_of(FAILURE_LOG_STRIDE)
                {
                    log::warning!(
                        "SNTP: synchronization still failing after {failures} attempts (last error {error:?})"
                    );
                }
                task::sleep(retry_delay(failures)).await;
            }
        }
    }
}

/// Query the configured servers in order and apply the first valid answer.
async fn attempt(
    client: &Client<'_>,
    configuration: &Configuration,
    states: &mut [ServerState],
    adapter: &mut Option<client::SntpUdpSocket>,
) -> Attempt {
    let mut last_error = Failure::Protocol(SntpError::Network);
    let mut any_usable = false;
    for (index, server) in configuration.servers.iter().enumerate() {
        if states[index].banned {
            continue;
        }
        any_usable = true;
        let result = async {
            let socket = client.ensure_socket(adapter).await.map_err(Failure::Io)?;
            client.exchange(server, socket).await
        }
        .await;
        match result {
            Ok(exchange) => match client.apply(&exchange) {
                Ok(applied) => {
                    return Attempt::Synchronized(Outcome {
                        server: server.clone(),
                        applied,
                        interval: states[index].interval(configuration.resync_interval),
                    });
                }
                Err(error) => last_error = error,
            },
            Err(Failure::Protocol(SntpError::KissOfDeath(code))) => {
                if let Some(message) = handle_kiss_of_death(&mut states[index], code) {
                    log::warning!("SNTP: server {server} {message}");
                }
                last_error = Failure::Protocol(SntpError::KissOfDeath(code));
            }
            Err(error) => last_error = error,
        }
    }
    if any_usable {
        Attempt::Failed(last_error)
    } else {
        Attempt::AllBanned
    }
}

fn log_success(outcome: &Outcome) {
    log::information!(
        "SNTP: UTC synchronized from {} (offset {:+} ms, round trip {} ms, accuracy {} ms)",
        outcome.server,
        outcome.applied.server_offset_micros as f64 / 1_000.0,
        outcome.applied.round_trip.as_millis(),
        outcome.applied.accuracy.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn retry_delay_doubles_and_is_bounded() {
        assert_eq!(retry_delay(1), INITIAL_RETRY_DELAY);
        assert_eq!(retry_delay(2), INITIAL_RETRY_DELAY * 2);
        assert_eq!(retry_delay(3), INITIAL_RETRY_DELAY * 4);
        assert_eq!(retry_delay(100), MAXIMUM_RETRY_DELAY);
        assert_eq!(retry_delay(u32::MAX), MAXIMUM_RETRY_DELAY);
        assert!(retry_delay(4) < MAXIMUM_RETRY_DELAY);
    }

    #[test]
    fn failure_logging_points() {
        let logged: Vec<u32> = (1..=25u32)
            .filter(|failures| {
                *failures >= REPEATED_FAILURE_LOG_THRESHOLD
                    && (failures - REPEATED_FAILURE_LOG_THRESHOLD)
                        .is_multiple_of(FAILURE_LOG_STRIDE)
            })
            .collect();
        assert_eq!(logged, vec![3, 13, 23]);
    }

    #[test]
    fn deny_and_rstr_ban_the_server_and_log_once() {
        for code in [KissOfDeathCode::Deny, KissOfDeathCode::Rstr] {
            let mut state = ServerState::default();
            let first = handle_kiss_of_death(&mut state, code);
            assert!(first.is_some(), "the first kiss of death is logged");
            assert!(state.banned);
            assert_eq!(
                state.interval(Duration::from_secs(3600)),
                Duration::from_secs(3600)
            );
            assert_eq!(handle_kiss_of_death(&mut state, code), None, "logged once");
        }
    }

    #[test]
    fn rate_doubles_the_interval_bounded_and_logs_once() {
        let mut state = ServerState::default();
        assert!(handle_kiss_of_death(&mut state, KissOfDeathCode::Rate).is_some());
        assert_eq!(
            state.interval(Duration::from_secs(3600)),
            Duration::from_secs(7200)
        );
        assert_eq!(
            handle_kiss_of_death(&mut state, KissOfDeathCode::Rate),
            None
        );
        assert_eq!(
            state.interval(Duration::from_secs(3600)),
            Duration::from_secs(14400)
        );
        for _ in 0..10 {
            handle_kiss_of_death(&mut state, KissOfDeathCode::Rate);
        }
        assert_eq!(
            state.interval(Duration::from_secs(3600)),
            Duration::from_secs(3600 * MAXIMUM_RATE_MULTIPLIER as u64)
        );
        assert!(!state.banned);
    }

    #[test]
    fn other_kiss_codes_change_nothing() {
        let mut state = ServerState::default();
        assert_eq!(
            handle_kiss_of_death(&mut state, KissOfDeathCode::Init),
            None
        );
        assert_eq!(state, ServerState::default());
    }
}
