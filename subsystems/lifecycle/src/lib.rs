//! OS lifecycle: shutdown, restart and sleep as an operating-system mechanism.
//!
//! Privileged executables call [`request`]; the platform runs a [`Supervisor`]
//! that stops tasks, commits storage, stops drivers and finally asks the
//! system power device (`/devices/power`, contract in `device::power`) to
//! perform the hardware action through the VFS. Shutdown itself is not a
//! device. [`Controller`] is the lock-free state machine behind it: exactly
//! one request wins and phases only move forward.
//!
//! The crate depends on no graphics stack, shell or particular filesystem.
#![no_std]

extern crate alloc;

mod supervisor;
pub mod tasks;

pub use supervisor::{Drivers, POWER_DEVICE_PATH, Supervisor};

use core::{
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};
use device::power::Capabilities;
use file_system::AccessFlags;
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

/// Final action of a lifecycle request.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Stop Xila and halt or power off (`device::power::POWER_OFF`).
    PowerOff = 1,
    /// Stop Xila and restart (`device::power::RESTART`).
    Restart = 2,
    /// Stop Xila and enter a low-power state (`device::power::SLEEP`).
    Sleep = 3,
}

impl Action {
    const fn from_bits(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::PowerOff),
            2 => Some(Self::Restart),
            3 => Some(Self::Sleep),
            _ => None,
        }
    }

    /// Power device capability needed to complete this action.
    pub const fn capability(self) -> Capabilities {
        match self {
            Self::PowerOff => Capabilities::POWER_OFF,
            Self::Restart => Capabilities::RESTART,
            Self::Sleep => Capabilities::SLEEP,
        }
    }
}

/// Lifecycle phase. Phases only move forward.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running = 0,
    /// A request was accepted; nothing has been stopped yet.
    Requested = 1,
    /// Tasks and guests were asked to stop and are being joined.
    Quiescing = 2,
    /// Writes are frozen and storage is being committed and unmounted.
    Syncing = 3,
    /// Drivers are being stopped.
    StoppingDrivers = 4,
    /// The power device is executing the final action.
    Finalizing = 5,
    /// The final action returned (for example a halted browser instance).
    Returned = 6,
    /// A stage failed. Resources stay allocated and the state is terminal.
    Failed = 7,
}

impl State {
    const fn from_bits(value: u16) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Requested,
            2 => Self::Quiescing,
            3 => Self::Syncing,
            4 => Self::StoppingDrivers,
            5 => Self::Finalizing,
            6 => Self::Returned,
            _ => Self::Failed,
        }
    }
}

/// Failure of a lifecycle request or pipeline stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The caller may not request lifecycle actions.
    PermissionDenied,
    /// Another request already won, or the pipeline already ran.
    Busy,
    /// Tasks did not stop before the deadline. They keep their resources.
    Timeout,
    /// Storage could not be frozen, committed or unmounted.
    Storage,
    /// A driver could not be stopped.
    Driver,
    /// The power device does not perform the requested action.
    Unsupported,
    /// The power device failed to perform the action.
    Power,
}

impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::PermissionDenied => "lifecycle request not permitted",
            Self::Busy => "lifecycle request already in progress",
            Self::Timeout => "tasks did not stop before the deadline",
            Self::Storage => "storage commit failed",
            Self::Driver => "driver stop failed",
            Self::Unsupported => "unsupported power action",
            Self::Power => "power action failed",
        })
    }
}

const STATE_MASK: u16 = 0x000f;
const ACTION_SHIFT: u16 = 4;
const STATUS_SHIFT: u16 = 8;

/// Lifecycle state machine. Phase, winning action and exit status share one
/// atomic word so a request is published as a whole.
pub struct Controller(AtomicU16);

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub const fn new() -> Self {
        Self(AtomicU16::new(State::Running as u16))
    }

    fn word(&self) -> u16 {
        self.0.load(Ordering::Acquire)
    }

    pub fn state(&self) -> State {
        State::from_bits(self.word() & STATE_MASK)
    }

    pub fn is_running(&self) -> bool {
        self.state() == State::Running
    }

    /// The winning action, once a request was accepted.
    pub fn action(&self) -> Option<Action> {
        Action::from_bits((self.word() >> ACTION_SHIFT) & 0xf)
    }

    /// Exit status of the winning request (meaningful to hosted targets).
    pub fn status(&self) -> u8 {
        (self.word() >> STATUS_SHIFT) as u8
    }

    /// Trusted request without privilege check, for platform code (for
    /// example host signal handlers). Executables use [`request`] instead.
    ///
    /// A single compare-and-exchange: safe in a POSIX signal handler on
    /// targets with native 16-bit atomics. It never allocates, locks or wakes;
    /// the supervisor polls [`Self::state`].
    pub fn request(&self, action: Action, status: u8) -> Result<(), State> {
        let word = (status as u16) << STATUS_SHIFT
            | (action as u16) << ACTION_SHIFT
            | State::Requested as u16;
        self.0
            .compare_exchange(
                State::Running as u16,
                word,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|word| State::from_bits(word & STATE_MASK))
    }

    fn advance(&self, from: State, to: State) -> Result<(), State> {
        let word = self.word();
        if word & STATE_MASK != from as u16 {
            return Err(State::from_bits(word & STATE_MASK));
        }
        self.0
            .compare_exchange(
                word,
                word & !STATE_MASK | to as u16,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|word| State::from_bits(word & STATE_MASK))
    }

    fn fail(&self, from: State) {
        let _ = self.advance(from, State::Failed);
    }

    /// Run the pipeline once for the accepted request.
    ///
    /// Any stage error moves to [`State::Failed`] and stops the pipeline: a
    /// failed storage commit never reaches drivers or the final action.
    /// Nothing is deallocated on failure. `budget` bounds quiescence on a
    /// monotonic clock.
    pub async fn run<S: Stages>(&self, stages: &mut S, budget: Duration) -> Result<(), Error> {
        self.execute(stages, budget, false).await.map(|_| ())
    }

    /// Dangerous: continue to the final action after a quiescence, storage or
    /// driver failure. Uncommitted data is lost and live tasks are cut off.
    /// Only for trusted platform code that explicitly accepts that loss.
    ///
    /// Returns the first skipped stage error if the final action returned.
    pub async fn run_forced<S: Stages>(
        &self,
        stages: &mut S,
        budget: Duration,
    ) -> Result<Option<Error>, Error> {
        self.execute(stages, budget, true).await
    }

    async fn execute<S: Stages>(
        &self,
        stages: &mut S,
        budget: Duration,
        force: bool,
    ) -> Result<Option<Error>, Error> {
        self.advance(State::Requested, State::Quiescing)
            .map_err(|_| Error::Busy)?;
        let action = self.action().ok_or(Error::Busy)?;
        if !stages.capabilities().contains(action.capability()) {
            self.fail(State::Quiescing);
            return Err(Error::Unsupported);
        }

        let mut skipped = None;
        let mut check = |state: State, result: Result<(), Error>| match result {
            Ok(()) => Ok(()),
            Err(error) if force => {
                skipped.get_or_insert(error);
                Ok(())
            }
            Err(error) => {
                self.fail(state);
                Err(error)
            }
        };

        check(State::Quiescing, stages.quiesce(budget).await)?;
        self.advance(State::Quiescing, State::Syncing)
            .map_err(|_| Error::Busy)?;
        check(State::Syncing, stages.storage().await)?;
        self.advance(State::Syncing, State::StoppingDrivers)
            .map_err(|_| Error::Busy)?;
        check(State::StoppingDrivers, stages.drivers().await)?;
        self.advance(State::StoppingDrivers, State::Finalizing)
            .map_err(|_| Error::Busy)?;
        if let Err(error) = stages.finalize(action, self.status()).await {
            self.fail(State::Finalizing);
            return Err(error);
        }
        self.advance(State::Finalizing, State::Returned)
            .map_err(|_| Error::Busy)?;
        Ok(skipped)
    }
}

/// Pipeline stages, executed in declaration order. [`Supervisor`] provides
/// the system implementation; tests and special runtimes may provide others.
#[allow(async_fn_in_trait)]
pub trait Stages {
    /// Power actions the final stage can perform.
    fn capabilities(&self) -> Capabilities;

    /// Reject new work, ask tasks and guests to stop and join them before a
    /// monotonic deadline `budget` from now. On timeout return
    /// [`Error::Timeout`] and keep every resource referenced by live tasks.
    async fn quiesce(&mut self, budget: Duration) -> Result<(), Error>;

    /// Freeze writes, require closed handles, flush, unmount secondary mounts
    /// before root and sync the backing devices.
    async fn storage(&mut self) -> Result<(), Error>;

    /// Stop drivers in dependency order without touching unmounted storage.
    async fn drivers(&mut self) -> Result<(), Error>;

    /// Perform the hardware action. May not return.
    async fn finalize(&mut self, action: Action, status: u8) -> Result<(), Error>;
}

/// The system-wide lifecycle controller.
pub static SYSTEM: Controller = Controller::new();

/// Request `action` on behalf of `task`.
///
/// The caller must be allowed to open the power device (`/devices/power`)
/// for writing: the platform's VFS permissions on that node are the privilege
/// policy (root and the administrator group on hosted platforms). The action
/// must be supported by the device. Accepting a request does not complete it:
/// the platform supervisor observes [`SYSTEM`] and runs the pipeline.
pub async fn request(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    action: Action,
    status: u8,
) -> Result<(), Error> {
    request_with(&SYSTEM, virtual_file_system, task, action, status).await
}

pub(crate) async fn request_with(
    controller: &Controller,
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    action: Action,
    status: u8,
) -> Result<(), Error> {
    use virtual_file_system::Error as VfsError;
    let mut device = File::open(
        virtual_file_system,
        task,
        POWER_DEVICE_PATH,
        AccessFlags::Write.into(),
    )
    .await
    .map_err(|error| match error {
        VfsError::PermissionDenied | VfsError::FileSystem(file_system::Error::PermissionDenied) => {
            Error::PermissionDenied
        }
        VfsError::RessourceBusy | VfsError::FileSystem(file_system::Error::RessourceBusy) => {
            Error::Busy
        }
        _ => Error::Unsupported,
    })?;
    let capabilities = device.control(device::power::GET_CAPABILITIES, &()).await;
    let _ = device.close(virtual_file_system).await;
    let capabilities = Capabilities(capabilities.map_err(|_| Error::Unsupported)?);
    if !capabilities.contains(action.capability()) {
        return Err(Error::Unsupported);
    }
    controller.request(action, status).map_err(|_| Error::Busy)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn concurrent_requests_have_exactly_one_winner() {
        let controller = Controller::new();
        let winners = std::thread::scope(|scope| {
            let threads: std::vec::Vec<_> = (0..16u8)
                .map(|index| {
                    let controller = &controller;
                    scope.spawn(move || {
                        let action = if index % 2 == 0 {
                            Action::PowerOff
                        } else {
                            Action::Restart
                        };
                        controller
                            .request(action, index)
                            .ok()
                            .map(|()| (action, index))
                    })
                })
                .collect();
            threads
                .into_iter()
                .filter_map(|thread| thread.join().unwrap())
                .collect::<std::vec::Vec<_>>()
        });
        assert_eq!(winners.len(), 1);
        assert_eq!(controller.state(), State::Requested);
        // Action and status come from the same winning request.
        assert_eq!(controller.action(), Some(winners[0].0));
        assert_eq!(controller.status(), winners[0].1);
    }

    #[test]
    fn states_never_rewind() {
        let controller = Controller::new();
        assert_eq!(
            controller.advance(State::Requested, State::Quiescing),
            Err(State::Running)
        );
        controller.request(Action::PowerOff, 143).unwrap();
        assert_eq!(
            controller.request(Action::Restart, 0),
            Err(State::Requested)
        );
        controller
            .advance(State::Requested, State::Quiescing)
            .unwrap();
        assert_eq!(
            controller.advance(State::Requested, State::Quiescing),
            Err(State::Quiescing)
        );
        controller.fail(State::Quiescing);
        assert_eq!(controller.request(Action::Restart, 0), Err(State::Failed));
        assert_eq!(
            controller.advance(State::Quiescing, State::Syncing),
            Err(State::Failed)
        );
        assert_eq!(controller.action(), Some(Action::PowerOff));
        assert_eq!(controller.status(), 143);
    }

    #[derive(Default)]
    struct Mock {
        calls: std::vec::Vec<&'static str>,
        fail: Option<(&'static str, Error)>,
        capabilities: Option<Capabilities>,
        finalized: Option<(Action, u8)>,
    }

    impl Mock {
        fn step(&mut self, name: &'static str) -> Result<(), Error> {
            self.calls.push(name);
            match self.fail {
                Some((stage, error)) if stage == name => Err(error),
                _ => Ok(()),
            }
        }
    }

    impl Stages for Mock {
        fn capabilities(&self) -> Capabilities {
            self.capabilities.unwrap_or(Capabilities::POWER_OFF)
        }
        async fn quiesce(&mut self, _: Duration) -> Result<(), Error> {
            self.step("quiesce")
        }
        async fn storage(&mut self) -> Result<(), Error> {
            self.step("storage")
        }
        async fn drivers(&mut self) -> Result<(), Error> {
            self.step("drivers")
        }
        async fn finalize(&mut self, action: Action, status: u8) -> Result<(), Error> {
            self.finalized = Some((action, status));
            self.step("finalize")
        }
    }

    fn ready<T>(future: impl Future<Output = T>) -> T {
        match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("mock stages never suspend"),
        }
    }

    fn requested(action: Action) -> Controller {
        let controller = Controller::new();
        controller.request(action, 0).unwrap();
        controller
    }

    const BUDGET: Duration = Duration::from_secs(5);

    #[test]
    fn success_runs_every_stage_once_in_order() {
        let controller = Controller::new();
        controller.request(Action::PowerOff, 3).unwrap();
        let mut mock = Mock::default();
        assert_eq!(ready(controller.run(&mut mock, BUDGET)), Ok(()));
        assert_eq!(mock.calls, ["quiesce", "storage", "drivers", "finalize"]);
        assert_eq!(mock.finalized, Some((Action::PowerOff, 3)));
        assert_eq!(controller.state(), State::Returned);
        assert_eq!(ready(controller.run(&mut mock, BUDGET)), Err(Error::Busy));
        assert_eq!(mock.calls.len(), 4);
    }

    #[test]
    fn run_without_request_is_rejected() {
        let controller = Controller::new();
        let mut mock = Mock::default();
        assert_eq!(ready(controller.run(&mut mock, BUDGET)), Err(Error::Busy));
        assert!(mock.calls.is_empty());
        assert_eq!(controller.state(), State::Running);
    }

    #[test]
    fn storage_failure_stops_before_drivers_and_power_action() {
        let controller = requested(Action::PowerOff);
        let mut mock = Mock {
            fail: Some(("storage", Error::Storage)),
            ..Default::default()
        };
        assert_eq!(
            ready(controller.run(&mut mock, BUDGET)),
            Err(Error::Storage)
        );
        assert_eq!(mock.calls, ["quiesce", "storage"]);
        assert_eq!(mock.finalized, None);
        assert_eq!(controller.state(), State::Failed);
        assert_eq!(ready(controller.run(&mut mock, BUDGET)), Err(Error::Busy));
    }

    #[test]
    fn quiescence_timeout_keeps_storage_and_drivers_untouched() {
        let controller = requested(Action::PowerOff);
        let mut mock = Mock {
            fail: Some(("quiesce", Error::Timeout)),
            ..Default::default()
        };
        assert_eq!(
            ready(controller.run(&mut mock, BUDGET)),
            Err(Error::Timeout)
        );
        assert_eq!(mock.calls, ["quiesce"]);
        assert_eq!(controller.state(), State::Failed);
    }

    #[test]
    fn unsupported_action_fails_before_quiescence() {
        let controller = requested(Action::Sleep);
        let mut mock = Mock::default();
        assert_eq!(
            ready(controller.run(&mut mock, BUDGET)),
            Err(Error::Unsupported)
        );
        assert!(mock.calls.is_empty());
        assert_eq!(controller.state(), State::Failed);
    }

    #[test]
    fn forced_run_reaches_the_power_action_and_reports_the_skipped_failure() {
        let controller = requested(Action::PowerOff);
        let mut mock = Mock {
            fail: Some(("storage", Error::Storage)),
            ..Default::default()
        };
        assert_eq!(
            ready(controller.run_forced(&mut mock, BUDGET)),
            Ok(Some(Error::Storage))
        );
        assert_eq!(mock.calls, ["quiesce", "storage", "drivers", "finalize"]);
        assert_eq!(controller.state(), State::Returned);
    }

    #[test]
    fn power_action_failure_is_terminal() {
        let controller = requested(Action::Restart);
        let mut mock = Mock {
            fail: Some(("finalize", Error::Power)),
            capabilities: Some(Capabilities::RESTART),
            ..Default::default()
        };
        assert_eq!(ready(controller.run(&mut mock, BUDGET)), Err(Error::Power));
        assert_eq!(controller.state(), State::Failed);
    }
}
