//! OS-wide shutdown coordination, independent of physical power control.
#![no_std]

use core::sync::atomic::{AtomicU8, Ordering};
use file_system::define_command;

pub const STATE_RUNNING: u8 = 0;
pub const STATE_REQUESTED: u8 = 1;
pub const STATE_STOPPING: u8 = 2;
pub const STATE_FINALIZING: u8 = 3;
pub const STATE_STOPPED: u8 = 4;
pub const STATE_FAILED: u8 = 5;

/// Read the lifecycle state byte without modifying system state.
define_command!(GET_SHUTDOWN_STATE, Read, b'P', 0, (), u8);
/// Request OS shutdown (not physical host power-off).
define_command!(REQUEST_SHUTDOWN, Write, b'P', 1, (), ());

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running = STATE_RUNNING,
    Requested = STATE_REQUESTED,
    Stopping = STATE_STOPPING,
    Finalizing = STATE_FINALIZING,
    Stopped = STATE_STOPPED,
    Failed = STATE_FAILED,
}

/// Lock-free, allocation-free state machine. Only the OS supervisor may advance
/// phases after Requested. A second caller cannot replace the first request.
pub struct Controller(AtomicU8);

impl Default for Controller {
    fn default() -> Self { Self::new() }
}

impl Controller {
    pub const fn new() -> Self { Self(AtomicU8::new(STATE_RUNNING)) }

    pub fn state(&self) -> State {
        match self.0.load(Ordering::Acquire) {
            STATE_RUNNING => State::Running,
            STATE_REQUESTED => State::Requested,
            STATE_STOPPING => State::Stopping,
            STATE_FINALIZING => State::Finalizing,
            STATE_STOPPED => State::Stopped,
            _ => State::Failed,
        }
    }

    /// Signal-handler safe: only an atomic compare-and-exchange.
    pub fn request_shutdown(&self) -> Result<(), State> {
        self.transition(State::Running, State::Requested)
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.state() != State::Running
    }

    pub fn begin_shutdown(&self) -> Result<(), State> {
        self.transition(State::Requested, State::Stopping)
    }

    pub fn begin_finalize(&self) -> Result<(), State> {
        self.transition(State::Stopping, State::Finalizing)
    }

    pub fn complete(&self) -> Result<(), State> {
        self.transition(State::Finalizing, State::Stopped)
    }

    pub fn fail(&self) {
        if self.is_shutdown_requested() {
            self.0.store(STATE_FAILED, Ordering::Release);
        }
    }

    fn transition(&self, from: State, to: State) -> Result<(), State> {
        self.0.compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| self.state())
    }
}

pub static SYSTEM: Controller = Controller::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_order_and_duplicate_request() {
        let state = Controller::new();
        assert_eq!(state.state(), State::Running);
        assert_eq!(state.begin_shutdown(), Err(State::Running));
        assert_eq!(state.request_shutdown(), Ok(()));
        assert_eq!(state.request_shutdown(), Err(State::Requested));
        assert!(state.is_shutdown_requested());
        assert_eq!(state.begin_shutdown(), Ok(()));
        assert_eq!(state.complete(), Err(State::Stopping));
        assert_eq!(state.begin_finalize(), Ok(()));
        assert_eq!(state.complete(), Ok(()));
        assert_eq!(state.state(), State::Stopped);
    }

    #[test]
    fn failure_does_not_reset_request() {
        let state = Controller::new();
        state.fail();
        assert_eq!(state.state(), State::Running);
        state.request_shutdown().unwrap();
        state.fail();
        assert_eq!(state.state(), State::Failed);
        assert_eq!(state.request_shutdown(), Err(State::Failed));
    }

    #[test]
    fn command_ids_are_distinct() {
        use file_system::ControlCommand;
        assert_ne!(GET_SHUTDOWN_STATE::IDENTIFIER, REQUEST_SHUTDOWN::IDENTIFIER);
    }
}
