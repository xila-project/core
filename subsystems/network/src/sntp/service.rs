//! The SNTP synchronization service: a task that starts when an interface has
//! an address, sets UTC from the configured servers through `sntpc`, and
//! resynchronizes periodically with a bounded exponential backoff on failure.
//!
//! Kiss-of-death answers (RFC 5905 section 7.4) get a per-server policy:
//! `DENY` and `RSTR` ban the server until the next configuration change or
//! reboot, and `RATE` sets a monotonic `not_before` deadline for that server
//! (now plus the doubled interval): servers whose deadline has not passed are
//! skipped, and the service sleeps until the earliest deadline. Each event is
//! logged once per server. A successful answer resets the server's rate
//! multiplier.
//!
//! After the first network synchronization, a clock step larger than 1000 s is
//! applied only when a second answer confirms it: the signed offset (candidate
//! minus the current clock) of the two answers must agree within 5 s. The
//! servers are tried in order, so a confirming answer from another server or
//! resolved address is preferred; with a single server, a later exchange of
//! the same server confirms, which means a single-server setup only resists a
//! blind off-path attacker. A pending step expires only after twice the larger
//! of the resynchronization interval and the maximum retry delay (30 min), so
//! the confirming attempt always comes before the expiry.
//!
//! Query times carry a random jitter, as the NTP Pool vendor guidance asks:
//! 0 to 2 s before the first query, and 0 to 12 percent added to every
//! resynchronization interval, retry delay and deferred wait. Resolved server
//! names are cached for one hour, and dropped after a failed exchange.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use sntpc::{Error as SntpError, KissOfDeathCode};
use task::{Manager as TaskManager, TaskIdentifier};
use time::Timestamp;

use crate::{
    DnsQueryKind, IntoIoErrorKind, IpAddress, Manager, Result,
    sntp::{
        client::{self, Applied, Client, Failure, MAXIMUM_CLOCK_STEP, STEP_CONFIRMATION_TOLERANCE},
        configuration::{self, Configuration, MAXIMUM_RESYNC_INTERVAL},
    },
};

/// How often to check that an interface has an address while waiting for the
/// network to come up.
pub(crate) const NETWORK_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// The delay before the first retry after a failure.
pub(crate) const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(60);

/// The largest delay between two attempts after failures.
pub(crate) const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(1800);

/// After this many consecutive failures, and then again every
/// `FAILURE_LOG_STRIDE`-th failure, a warning is logged.
pub(crate) const REPEATED_FAILURE_LOG_THRESHOLD: u32 = 3;

/// The stride between two failure warnings once the threshold is reached.
pub(crate) const FAILURE_LOG_STRIDE: u32 = 10;

/// The largest multiplier a `RATE` kiss of death applies to one server's
/// resynchronization interval.
pub(crate) const MAXIMUM_RATE_MULTIPLIER: u32 = 64;

/// How long a resolved server name is reused before resolving it again.
pub(crate) const DNS_CACHE_LIFETIME: Duration = Duration::from_secs(3600);

/// Random delay before the first query, from zero up to this bound.
pub(crate) const FIRST_QUERY_JITTER: Duration = Duration::from_secs(2);

/// Random percentage added to every interval, retry delay and deferred wait,
/// from 0 up to this value.
pub(crate) const JITTER_PERCENT: u64 = 12;

/// The delay before retrying after `failures` consecutive failures: the
/// initial delay doubled per failure, capped at the maximum.
pub(crate) fn retry_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(16);
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let millis = (INITIAL_RETRY_DELAY.as_millis() as u64)
        .saturating_mul(factor)
        .min(MAXIMUM_RETRY_DELAY.as_millis() as u64);
    Duration::from_millis(millis)
}

/// A random duration from zero up to `span`, from the network manager's random
/// device; zero when the device cannot be read.
fn jitter(network: &Manager, span: Duration) -> Duration {
    match network.random_u32() {
        Some(value) => {
            let nanos =
                (u128::from(value) * span.as_nanos() / u128::from(u32::MAX)).min(u64::MAX as u128);
            Duration::from_nanos(nanos as u64)
        }
        None => Duration::ZERO,
    }
}

/// `base` plus a random 0 to `JITTER_PERCENT` percent.
fn add_jitter(network: &Manager, base: Duration) -> Duration {
    let percent = network
        .random_u32()
        .map(|value| u64::from(value) % (JITTER_PERCENT + 1))
        .unwrap_or(0);
    let nanos = base.as_nanos().saturating_mul(u128::from(100 + percent)) / 100;
    Duration::from_nanos(nanos.min(u64::MAX as u128) as u64)
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
    /// Monotonic instant before which a `RATE` kiss of death says the server
    /// must not be queried.
    not_before: Option<Duration>,
}

impl ServerState {
    /// The resynchronization interval of this server: the configured interval
    /// times the rate multiplier, saturated.
    fn interval(&self, base: Duration) -> Duration {
        base.checked_mul(self.rate_multiplier.max(1))
            .unwrap_or(MAXIMUM_RESYNC_INTERVAL)
    }

    /// Whether the server may not be queried at the monotonic instant `now`.
    fn blocked(&self, now: Duration) -> bool {
        self.banned || self.not_before.is_some_and(|deadline| now < deadline)
    }
}

/// The earliest monotonic deadline among the rate-limited servers that are not
/// yet due, if any.
fn earliest_deadline(states: &[ServerState], now: Duration) -> Option<Duration> {
    states
        .iter()
        .filter(|state| !state.banned)
        .filter_map(|state| state.not_before)
        .filter(|deadline| now < *deadline)
        .min()
}

/// Apply a kiss-of-death answer to one server's state and log it once. Returns
/// whether this call logged it. `now` is the monotonic instant and `base` the
/// configured resynchronization interval.
pub(crate) fn handle_kiss_of_death(
    state: &mut ServerState,
    code: KissOfDeathCode,
    server: &str,
    now: Duration,
    base: Duration,
) -> bool {
    match code {
        KissOfDeathCode::Deny | KissOfDeathCode::Rstr => {
            state.banned = true;
            if state.kiss_logged {
                return false;
            }
            state.kiss_logged = true;
            log::warning!(
                "SNTP: server {server} denied access ({}); not used again until the next configuration change or reboot",
                code.as_str()
            );
            true
        }
        KissOfDeathCode::Rate => {
            state.rate_multiplier = state
                .rate_multiplier
                .max(1)
                .saturating_mul(2)
                .min(MAXIMUM_RATE_MULTIPLIER);
            let interval = state.interval(base);
            state.not_before = Some(now.checked_add(interval).unwrap_or(Duration::MAX));
            if state.kiss_logged {
                return false;
            }
            state.kiss_logged = true;
            log::warning!(
                "SNTP: server {server} asked to slow down ({}); next query in {} s",
                code.as_str(),
                interval.as_secs()
            );
            true
        }
        _ => false,
    }
}

/// One successful synchronization.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The server that answered.
    pub server: &'static str,
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
    /// Every usable server is rate-limited: wait until this monotonic deadline.
    Deferred(Duration),
    /// A clock step larger than the bound was seen but no second answer
    /// confirmed it yet.
    Unconfirmed,
}

/// A large clock step waiting for a confirming second answer: the signed
/// offset in nanoseconds (candidate minus the current clock), the server that
/// reported it, the address that answered, and the monotonic instant when it
/// was recorded. It expires after [`pending_step_age_limit`].
#[derive(Clone, Debug)]
struct PendingStep {
    offset_nanos: i128,
    server: usize,
    address: Option<IpAddress>,
    recorded_at: Duration,
}

/// The age limit of a pending step: the larger of the resynchronization
/// interval and the maximum retry delay, times two. After an unconfirmed step
/// the loop waits a retry delay (60 s first, doubling up to 30 min), so the
/// limit must exceed the longest wait before the confirming attempt: with the
/// smallest interval (60 s) a limit of one interval expired the step before
/// the next attempt and a single server could never confirm.
fn pending_step_age_limit(configuration: &Configuration) -> Duration {
    configuration
        .resync_interval
        .max(MAXIMUM_RETRY_DELAY)
        .saturating_mul(2)
}

/// Whether a pending step recorded at `recorded_at` is expired at `now`.
fn pending_step_expired(pending: &PendingStep, now: Duration, limit: Duration) -> bool {
    now.saturating_sub(pending.recorded_at) > limit
}

/// A cached name resolution.
struct DnsEntry {
    name: String,
    addresses: Vec<IpAddress>,
    resolved_at: Duration,
}

/// Resolve `name`, reusing the cached addresses while they are younger than
/// [`DNS_CACHE_LIFETIME`].
async fn resolve_cached(
    network: &Manager,
    cache: &mut Vec<DnsEntry>,
    name: &str,
    now: Duration,
) -> core::result::Result<Vec<IpAddress>, Failure> {
    if let Some(entry) = cache.iter().find(|entry| entry.name == name)
        && now.saturating_sub(entry.resolved_at) < DNS_CACHE_LIFETIME
    {
        return Ok(entry.addresses.clone());
    }
    let addresses = network
        .resolve(name, DnsQueryKind::A | DnsQueryKind::Aaaa, true, None)
        .await
        .map_err(|error| Failure::Io(error.kind()))?;
    match cache.iter_mut().find(|entry| entry.name == name) {
        Some(entry) => {
            entry.addresses = addresses.clone();
            entry.resolved_at = now;
        }
        None => cache.push(DnsEntry {
            name: String::from(name),
            addresses: addresses.clone(),
            resolved_at: now,
        }),
    }
    Ok(addresses)
}

/// Drop the cached resolution of `name`, for example after a failed exchange:
/// the next attempt resolves it again.
fn drop_cache_entry(cache: &mut Vec<DnsEntry>, name: &str) {
    cache.retain(|entry| entry.name != name);
}

/// The SNTP service, assembled from the runtime subsystems it needs.
pub struct Service {
    task_manager: &'static TaskManager,
    network: &'static Manager,
    time: &'static time::Manager<'static>,
    virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
    minimum_timestamp: Timestamp,
}

impl Service {
    /// `minimum_timestamp` is the platform's floor for UTC, as Unix time in
    /// UTC: on a device without a battery clock, the build date, for example
    /// `time::Timestamp::from(core::time::Duration::from_secs(BUILD_EPOCH))`.
    /// Answers before it are rejected.
    pub fn new(
        task_manager: &'static TaskManager,
        network: &'static Manager,
        time: &'static time::Manager<'static>,
        virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
        minimum_timestamp: Timestamp,
    ) -> Self {
        Self {
            task_manager,
            network,
            time,
            virtual_file_system,
            minimum_timestamp,
        }
    }

    /// Load the configuration and spawn the service task. The task runs until
    /// the system stops; its identifier is returned.
    pub async fn start(self, task: TaskIdentifier) -> Result<TaskIdentifier> {
        self.start_on(task, None).await
    }

    /// Start the service on a selected executor. With no selection, use the
    /// executor that calls this method.
    pub async fn start_on(
        self,
        task: TaskIdentifier,
        executor: Option<task::SpawnerIdentifier>,
    ) -> Result<TaskIdentifier> {
        // The configuration is allocated once, outside the task stack.
        let configuration: &'static Configuration = Box::leak(Box::new(
            configuration::load(self.virtual_file_system, task).await,
        ));
        let network = self.network;
        let time = self.time;
        let minimum_timestamp = self.minimum_timestamp;
        let service = move |_| async move {
            run(network, time, configuration, minimum_timestamp).await;
        };
        let name = "SNTP synchronization";
        match executor {
            Some(executor) => {
                self.task_manager
                    .spawn_on_executor(task, name, executor, service)
                    .await
            }
            None => self.task_manager.spawn(task, name, None, service).await,
        }
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
    minimum_timestamp: Timestamp,
) {
    let client = Client::new(network, time, Some(minimum_timestamp));
    let mut adapter: Option<client::SntpUdpSocket> = None;
    let mut states: Vec<ServerState> = configuration
        .servers
        .iter()
        .map(|_| ServerState::default())
        .collect();
    let mut dns_cache: Vec<DnsEntry> = Vec::new();
    let mut pending_step: Option<PendingStep> = None;
    let mut failures: u32 = 0;
    let mut synchronized_once = false;
    let mut first_query = true;

    loop {
        // Start when an interface has an address.
        while !network.has_ip_address(None).await {
            task::sleep(NETWORK_POLL_INTERVAL).await;
        }
        if first_query {
            first_query = false;
            task::sleep(jitter(network, FIRST_QUERY_JITTER)).await;
        }

        match attempt(
            &client,
            network,
            time,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await
        {
            Attempt::Synchronized(outcome) => {
                if !synchronized_once {
                    synchronized_once = true;
                    log_success(&outcome);
                }
                failures = 0;
                task::sleep(add_jitter(network, outcome.interval)).await;
            }
            Attempt::AllBanned => {
                // Nothing is queried until the next configuration change or
                // reboot; the kiss-of-death was already logged once.
                task::sleep(add_jitter(network, configuration.resync_interval)).await;
            }
            Attempt::Deferred(deadline) => {
                // Every usable server is rate-limited: wait for the earliest
                // deadline.
                let wait = deadline.saturating_sub(time.monotonic_now());
                task::sleep(add_jitter(network, wait)).await;
            }
            Attempt::Unconfirmed => {
                failures = failures.saturating_add(1);
                log::warning!(
                    "SNTP: a clock step larger than {} s is waiting for a confirming second answer",
                    MAXIMUM_CLOCK_STEP.as_secs()
                );
                task::sleep(add_jitter(network, retry_delay(failures))).await;
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
                task::sleep(add_jitter(network, retry_delay(failures))).await;
            }
        }
    }
}

/// Query the configured servers in order and apply the first valid answer.
/// Servers that are banned or rate-limited are skipped.
#[allow(clippy::too_many_arguments)]
async fn attempt(
    client: &Client<'_>,
    network: &Manager,
    time: &time::Manager<'_>,
    configuration: &'static Configuration,
    states: &mut [ServerState],
    adapter: &mut Option<client::SntpUdpSocket>,
    dns_cache: &mut Vec<DnsEntry>,
    pending_step: &mut Option<PendingStep>,
) -> Attempt {
    let now = time.monotonic_now();
    // A pending step expires after the age limit, which is always longer than
    // the wait before the confirming attempt.
    let age_limit = pending_step_age_limit(configuration);
    if let Some(pending) = pending_step.as_ref()
        && pending_step_expired(pending, now, age_limit)
    {
        *pending_step = None;
    }
    let mut last_error = Failure::Protocol(SntpError::Network);
    let mut queried = false;
    let mut unconfirmed = false;
    for (index, server) in configuration.servers.iter().enumerate() {
        if states[index].blocked(now) {
            continue;
        }
        queried = true;
        let addresses = match resolve_cached(network, dns_cache, server, now).await {
            Ok(addresses) => addresses,
            Err(error) => {
                last_error = error;
                continue;
            }
        };
        let result = match client.ensure_socket(adapter).await {
            Ok(socket) => client.exchange_addresses(&addresses, socket).await,
            Err(error) => Err(Failure::Io(error)),
        };
        match result {
            Ok(exchange) => {
                let candidate = match client.candidate(&exchange) {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        last_error = error;
                        continue;
                    }
                };
                // After the first network synchronization, a step larger than
                // the bound is applied only when a second answer confirms it:
                // the signed offsets of the two answers must agree.
                let already_network = time
                    .status()
                    .synchronization
                    .is_some_and(|status| status.source == time::ClockSource::Network);
                let offset_nanos = if already_network {
                    time.realtime_now()
                        .ok()
                        .map(|current| candidate.as_nanos() - current.as_nanos())
                } else {
                    None
                };
                match offset_nanos {
                    Some(offset_nanos)
                        if offset_nanos.abs() > MAXIMUM_CLOCK_STEP.as_nanos() as i128 =>
                    {
                        let confirmed = pending_step.as_ref().is_some_and(|pending| {
                            (offset_nanos - pending.offset_nanos).abs()
                                <= STEP_CONFIRMATION_TOLERANCE.as_nanos() as i128
                        });
                        if confirmed {
                            if let Some(pending) = pending_step.take()
                                && (pending.server != index
                                    || pending.address != Some(exchange.answered_from.clone()))
                            {
                                log::information!(
                                    "SNTP: the large clock step is confirmed by a second server or address"
                                );
                            }
                        } else {
                            *pending_step = Some(PendingStep {
                                offset_nanos,
                                server: index,
                                address: Some(exchange.answered_from.clone()),
                                recorded_at: now,
                            });
                            unconfirmed = true;
                            last_error = Failure::Protocol(SntpError::InvalidTimestamp);
                            continue;
                        }
                    }
                    // Any other answer (in range, or before the first network
                    // synchronization) clears the pending step, so a stale
                    // step can never confirm a later answer.
                    _ => *pending_step = None,
                }
                match client.apply(&exchange) {
                    Ok(applied) => {
                        // A success clears the rate multiplier of the server
                        // that answered.
                        states[index].rate_multiplier = 1;
                        return Attempt::Synchronized(Outcome {
                            server: &configuration.servers[index],
                            applied,
                            interval: states[index].interval(configuration.resync_interval),
                        });
                    }
                    Err(error) => last_error = error,
                }
            }
            Err(Failure::Protocol(SntpError::KissOfDeath(code))) => {
                handle_kiss_of_death(
                    &mut states[index],
                    code,
                    server,
                    time.monotonic_now(),
                    configuration.resync_interval,
                );
                last_error = Failure::Protocol(SntpError::KissOfDeath(code));
            }
            Err(error) => {
                // A failed or timed-out exchange: resolve the name again on
                // the next attempt.
                drop_cache_entry(dns_cache, server);
                last_error = error;
            }
        }
    }
    if unconfirmed {
        Attempt::Unconfirmed
    } else if queried {
        Attempt::Failed(last_error)
    } else if let Some(deadline) = earliest_deadline(states, now) {
        Attempt::Deferred(deadline)
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
    use crate::sntp::support::{
        TEST_MUTEX, host_time_manager, spawn_scripted_server, valid_answer,
    };
    use crate::tests::initialize;
    use alloc::vec;
    use core::cell::Cell;
    use time::{ClockSource, RealtimeSample};

    /// 2027-01-01 00:00:00 UTC, in Unix nanoseconds.
    const T0: i128 = 1_800_000_000_000_000_000;

    /// A static configuration with the given server names and a one-hour
    /// resynchronization interval.
    fn test_configuration(servers: &[&str]) -> &'static Configuration {
        Box::leak(Box::new(Configuration {
            servers: servers.iter().map(|name| String::from(*name)).collect(),
            resync_interval: Duration::from_secs(3600),
        }))
    }

    /// Set the clock to `nanos`, claiming a network synchronization.
    fn set_network_clock(time: &time::Manager<'_>, nanos: i128) {
        time.set_realtime(RealtimeSample {
            timestamp: Timestamp::from_nanos(nanos),
            source: ClockSource::Network,
            resolution: Duration::from_secs(1),
            accuracy: None,
        })
        .unwrap();
    }

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
        let base = Duration::from_secs(3600);
        for code in [KissOfDeathCode::Deny, KissOfDeathCode::Rstr] {
            let mut state = ServerState::default();
            assert!(handle_kiss_of_death(
                &mut state,
                code,
                "a",
                Duration::ZERO,
                base
            ));
            assert!(state.banned);
            assert_eq!(state.interval(base), base);
            assert!(state.blocked(Duration::ZERO));
            assert!(!handle_kiss_of_death(
                &mut state,
                code,
                "a",
                Duration::ZERO,
                base
            ));
        }
    }

    #[test]
    fn rate_sets_a_monotonic_deadline_and_doubles_the_interval_bounded() {
        let base = Duration::from_secs(3600);
        let now = Duration::from_secs(1000);
        let mut state = ServerState::default();
        assert!(handle_kiss_of_death(
            &mut state,
            KissOfDeathCode::Rate,
            "a",
            now,
            base
        ));
        // The next query is not before now plus the doubled interval.
        assert_eq!(state.not_before, Some(now + base * 2));
        assert_eq!(state.interval(base), base * 2);
        assert!(state.blocked(now + base * 2 - Duration::from_secs(1)));
        assert!(!state.blocked(now + base * 2));
        // A second RATE moves the deadline again but is not logged again.
        assert!(!handle_kiss_of_death(
            &mut state,
            KissOfDeathCode::Rate,
            "a",
            now,
            base
        ));
        assert_eq!(state.not_before, Some(now + base * 4));
        assert_eq!(state.interval(base), base * 4);
        for _ in 0..10 {
            handle_kiss_of_death(&mut state, KissOfDeathCode::Rate, "a", now, base);
        }
        assert_eq!(state.interval(base), base * MAXIMUM_RATE_MULTIPLIER);
        assert_eq!(state.not_before, Some(now + base * MAXIMUM_RATE_MULTIPLIER));
        assert!(!state.banned);
    }

    #[test]
    fn with_two_servers_only_the_rate_limited_one_is_skipped() {
        let base = Duration::from_secs(3600);
        let now = Duration::from_secs(1000);
        let mut states = vec![ServerState::default(), ServerState::default()];
        assert!(handle_kiss_of_death(
            &mut states[0],
            KissOfDeathCode::Rate,
            "a",
            now,
            base
        ));
        assert!(states[0].blocked(now), "server 0 waits for its deadline");
        assert!(!states[1].blocked(now), "server 1 is still queried");
        assert_eq!(earliest_deadline(&states, now), Some(now + base * 2));
        // Once both are limited, the earliest deadline wins.
        let later = now + base * 2;
        assert!(handle_kiss_of_death(
            &mut states[1],
            KissOfDeathCode::Rate,
            "b",
            later,
            base
        ));
        assert_eq!(earliest_deadline(&states, later), Some(later + base * 2));
        assert_eq!(earliest_deadline(&states, later + base * 2), None);
    }

    #[test]
    fn other_kiss_codes_change_nothing() {
        let mut state = ServerState::default();
        assert!(!handle_kiss_of_death(
            &mut state,
            KissOfDeathCode::Init,
            "a",
            Duration::ZERO,
            Duration::from_secs(3600)
        ));
        assert_eq!(state, ServerState::default());
    }

    /// An honest server that is 2000 s ahead of the (network-synchronized but
    /// wrong) local clock: its two answers carry the same signed offset, so
    /// the second one confirms the large step and applies it.
    #[task::test]
    async fn an_honest_large_step_is_confirmed_by_a_second_answer() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        set_network_clock(time_manager, T0);
        let client = Client::new(network, time_manager, None);

        // The honest server answers "now plus 2000 s" at answer time.
        let (ready, server_task) = spawn_scripted_server(
            [127, 0, 0, 1],
            move |request| {
                let now = time_manager.realtime_now().unwrap().as_nanos();
                alloc::vec![valid_answer(now + 2_000_000_000_000)(request)]
            },
            2,
        )
        .await;
        ready.wait().await;

        let configuration = test_configuration(&["127.0.0.1"]);
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(first, Attempt::Unconfirmed),
            "the first answer alone does not apply a 2000 s step: {first:?}"
        );
        let pending = pending_step.as_ref().expect("a step is pending");
        assert!(
            (pending.offset_nanos - 2_000_000_000_000).abs() < 1_000_000_000,
            "the pending step is the signed offset: {}",
            pending.offset_nanos
        );

        // Ten seconds pass: the local clock advances and the honest server
        // follows it, so the offset stays the same.
        set_network_clock(time_manager, T0 + 10_000_000_000);
        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(second, Attempt::Synchronized(_)),
            "the second answer confirms the step: {second:?}"
        );
        assert!(pending_step.is_none());
        server_task.join().await;
    }

    /// A server repeating one constant wrong time: the signed offset moves
    /// with the local clock, so the second answer does not confirm it.
    #[task::test]
    async fn a_constant_wrong_time_is_not_confirmed() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        set_network_clock(time_manager, T0);
        let client = Client::new(network, time_manager, None);

        // The attacker always answers the same frozen time, 5000 s ahead.
        let constant = T0 + 5_000_000_000_000;
        let (ready, server_task) = spawn_scripted_server(
            [127, 0, 0, 1],
            move |request| alloc::vec![valid_answer(constant)(request)],
            2,
        )
        .await;
        ready.wait().await;

        let configuration = test_configuration(&["127.0.0.1"]);
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(matches!(first, Attempt::Unconfirmed), "{first:?}");

        // Ten seconds pass: the constant answer is now 10 s further from the
        // local clock, outside the confirmation tolerance.
        set_network_clock(time_manager, T0 + 10_000_000_000);
        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(second, Attempt::Unconfirmed),
            "a constant wrong time never confirms: {second:?}"
        );
        // The clock was not set to the wrong time.
        let realtime = time_manager.realtime_now().unwrap().as_nanos();
        assert!(
            (realtime - (T0 + 10_000_000_000)).abs() < 1_000_000_000,
            "the clock kept its own time: {realtime}"
        );
        server_task.join().await;
    }

    /// An in-range answer applies and clears the pending step, so a single
    /// later large answer does not confirm at once.
    #[task::test]
    async fn an_in_range_answer_clears_the_pending_step() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        set_network_clock(time_manager, T0);
        let client = Client::new(network, time_manager, None);

        // The server answers +2000 s, then in range, then +2000 s again.
        let calls = Cell::new(0usize);
        let (ready, server_task) = spawn_scripted_server(
            [127, 0, 0, 1],
            move |request| {
                let call = calls.get();
                calls.set(call + 1);
                let now = time_manager.realtime_now().unwrap().as_nanos();
                let offset = if call == 1 { 0 } else { 2000 };
                alloc::vec![valid_answer(now + offset * 1_000_000_000)(request)]
            },
            3,
        )
        .await;
        ready.wait().await;

        let configuration = test_configuration(&["127.0.0.1"]);
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(matches!(first, Attempt::Unconfirmed), "{first:?}");
        assert!(pending_step.is_some());

        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(second, Attempt::Synchronized(_)),
            "the in-range answer applies: {second:?}"
        );
        assert!(
            pending_step.is_none(),
            "the applied answer cleared the pending step"
        );

        let third = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(third, Attempt::Unconfirmed),
            "a single later large answer does not confirm without a pending step: {third:?}"
        );
        server_task.join().await;
    }

    /// The pending-step age limit is independent of the retry race: at the
    /// smallest interval (60 s) it is twice the maximum retry delay, so a step
    /// recorded one retry delay ago is not expired.
    #[test]
    fn the_pending_step_age_limit_beats_the_retry_race() {
        let minimum = Configuration {
            servers: vec![String::from("a")],
            resync_interval: Duration::from_secs(60),
        };
        let limit = pending_step_age_limit(&minimum);
        assert_eq!(limit, MAXIMUM_RETRY_DELAY * 2);
        assert!(
            limit > retry_delay(1),
            "the first retry delay comes before the expiry"
        );
        assert!(
            limit > MAXIMUM_RETRY_DELAY,
            "the longest retry delay comes before the expiry"
        );
        let pending = PendingStep {
            offset_nanos: 2_000_000_000_000,
            server: 0,
            address: None,
            recorded_at: Duration::ZERO,
        };
        assert!(!pending_step_expired(&pending, retry_delay(1), limit));
        assert!(!pending_step_expired(&pending, MAXIMUM_RETRY_DELAY, limit));
        assert!(!pending_step_expired(&pending, limit, limit));
        assert!(pending_step_expired(
            &pending,
            limit + Duration::from_secs(1),
            limit
        ));
        // At a larger interval the limit follows the interval, saturated.
        let large = Configuration {
            servers: vec![String::from("a")],
            resync_interval: Duration::from_secs(7 * 24 * 3600),
        };
        assert_eq!(
            pending_step_age_limit(&large),
            Duration::from_secs(2 * 7 * 24 * 3600)
        );
    }

    /// At the minimum resynchronization interval (60 s), a +2000 s step is
    /// confirmed by the next attempt: the age limit (twice the maximum retry
    /// delay) is far longer than the wait before that attempt.
    #[task::test]
    async fn a_large_step_confirms_at_the_minimum_interval() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        set_network_clock(time_manager, T0);
        let client = Client::new(network, time_manager, None);

        // The server always answers 2000 s ahead of the current clock.
        let (ready, server_task) = spawn_scripted_server(
            [127, 0, 0, 1],
            move |request| {
                let now = time_manager.realtime_now().unwrap().as_nanos();
                alloc::vec![valid_answer(now + 2_000_000_000_000)(request)]
            },
            2,
        )
        .await;
        ready.wait().await;

        // resync_interval_seconds = 60, the clamped minimum.
        let configuration = Box::leak(Box::new(Configuration {
            servers: vec![String::from("127.0.0.1")],
            resync_interval: Duration::from_secs(60),
        }));
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(matches!(first, Attempt::Unconfirmed), "{first:?}");
        assert!(pending_step.is_some());

        // The next attempt (after one retry delay in the real loop) confirms.
        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(second, Attempt::Synchronized(_)),
            "the step confirms at the minimum interval: {second:?}"
        );
        assert!(pending_step.is_none());
        server_task.join().await;
    }

    /// A RATE kiss of death defers the next attempt until the deadline.
    #[task::test]
    async fn a_rate_kiss_defers_the_next_attempt() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network, time_manager, None);

        let kod = |request: [u8; 48]| {
            let mut response = [0u8; 48];
            response[0] = 0x24 | (3 << 6);
            response[1] = 0; // stratum 0: kiss of death
            response[12..16].copy_from_slice(b"RATE");
            response[24..32].copy_from_slice(&request[40..48]);
            response[40..48].copy_from_slice(&request[40..48]);
            alloc::vec![response]
        };
        let (ready, server_task) = spawn_scripted_server([127, 0, 0, 1], kod, 1).await;
        ready.wait().await;

        let configuration = test_configuration(&["127.0.0.1"]);
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(
            matches!(
                first,
                Attempt::Failed(Failure::Protocol(SntpError::KissOfDeath(
                    KissOfDeathCode::Rate
                )))
            ),
            "{first:?}"
        );
        server_task.join().await;

        let before = time_manager.monotonic_now();
        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        match second {
            Attempt::Deferred(deadline) => {
                assert!(deadline > before, "the deadline is in the future");
                assert_eq!(deadline, states[0].not_before.unwrap());
                // Roughly the doubled interval from the kiss of death.
                assert!(deadline - before >= Duration::from_secs(7199));
            }
            other => panic!("expected a deferred attempt, got {other:?}"),
        }
    }

    /// A failed exchange drops the cached resolution of the name; a successful
    /// one keeps it.
    #[task::test]
    async fn a_failed_exchange_drops_the_dns_cache_entry() {
        let _lock = TEST_MUTEX.lock().await;
        let network = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network, time_manager, None);

        // The first answer is garbage (mode 5), the second is valid.
        let calls = Cell::new(0usize);
        let (ready, server_task) = spawn_scripted_server(
            [127, 0, 0, 1],
            move |request| {
                if calls.get() == 0 {
                    calls.set(1);
                    let mut response = valid_answer(T0)(request);
                    response[0] = 0x25; // mode 5: rejected by sntpc
                    alloc::vec![response]
                } else {
                    alloc::vec![valid_answer(T0)(request)]
                }
            },
            2,
        )
        .await;
        ready.wait().await;

        let configuration = test_configuration(&["127.0.0.1"]);
        let mut states = vec![ServerState::default()];
        let mut adapter = None;
        let mut dns_cache: Vec<DnsEntry> = Vec::new();
        let mut pending_step: Option<PendingStep> = None;

        let first = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(matches!(first, Attempt::Failed(_)), "{first:?}");
        assert!(
            dns_cache.is_empty(),
            "the failed exchange dropped the cached resolution"
        );

        let second = attempt(
            &client,
            network,
            time_manager,
            configuration,
            &mut states,
            &mut adapter,
            &mut dns_cache,
            &mut pending_step,
        )
        .await;
        assert!(matches!(second, Attempt::Synchronized(_)), "{second:?}");
        assert_eq!(dns_cache.len(), 1, "the success cached the resolution");
        server_task.join().await;
    }
}
