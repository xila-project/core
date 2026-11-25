//! The SNTP synchronization service: a task that starts when an interface has
//! an address, sets UTC from the configured servers, and resynchronizes
//! periodically with a bounded exponential backoff on failure.

use alloc::boxed::Box;
use core::time::Duration;

use task::{Manager as TaskManager, TaskIdentifier};

use crate::{
    IntoIoErrorKind, IoErrorKind, Manager, Result, UdpSocket,
    sntp::{
        client::{self, Client},
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

/// One successful synchronization.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The server that answered.
    pub server: configuration::ServerName,
    /// What applying the answer changed.
    pub applied: client::Applied,
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
        // The configuration is allocated once, outside the task stack: it is
        // over a kilobyte with the default server list capacity.
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
    let mut socket: Option<UdpSocket> = None;
    let mut failures: u32 = 0;
    let mut synchronized_once = false;

    loop {
        // Start when an interface has an address.
        while !network.has_ip_address(None).await {
            task::sleep(NETWORK_POLL_INTERVAL).await;
        }

        match attempt(&client, configuration, &mut socket).await {
            Ok(outcome) => {
                if !synchronized_once {
                    synchronized_once = true;
                    log_success(&outcome);
                }
                failures = 0;
                task::sleep(configuration.resync_interval).await;
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                if client::invalidates_socket(error) {
                    // The interface may have changed: rebuild the socket.
                    socket = None;
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
    socket: &mut Option<UdpSocket>,
) -> Result<Outcome> {
    let mut last_error = IoErrorKind::NetworkDown;
    for server in &configuration.servers {
        let result = async {
            let socket = client.ensure_socket(socket).await?;
            client.exchange(server.as_str(), socket).await
        }
        .await;
        match result {
            Ok(timing) => match client.apply(&timing) {
                Ok(applied) => {
                    return Ok(Outcome {
                        server: server.clone(),
                        applied,
                    });
                }
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn log_success(outcome: &Outcome) {
    let offset_millis = outcome
        .applied
        .offset_nanos
        .map(|nanos| nanos / 1_000_000)
        .unwrap_or(0);
    log::information!(
        "SNTP: UTC synchronized from {} (offset {:+} ms, round trip {} ms, accuracy {} ms)",
        outcome.server.as_str(),
        offset_millis,
        outcome.applied.round_trip.as_millis(),
        outcome.applied.accuracy.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

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
        assert_eq!(logged, [3, 13, 23]);
    }
}
