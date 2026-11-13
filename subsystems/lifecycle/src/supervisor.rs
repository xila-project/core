//! System shutdown pipeline over the task manager, the VFS and the power device.

use crate::{Action, Controller, Error, Stages, tasks};
use core::time::Duration;
use device::power::{Capabilities, GET_CAPABILITIES, POWER_OFF, RESTART, SLEEP};
use file_system::AccessFlags;
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

/// System power device node, mounted by the platform.
pub const POWER_DEVICE_PATH: &str = "/devices/power";

/// Platform driver stop hook, run after storage was committed and unmounted.
#[allow(async_fn_in_trait)]
pub trait Drivers {
    /// Stop drivers in dependency order. Must not use unmounted storage.
    async fn stop(&mut self) -> Result<(), Error>;
}

/// No driver needs an explicit stop.
impl Drivers for () {
    async fn stop(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// Runs the pipeline for a platform: quiesce the descendants of `task`
/// (except `services`), commit storage, stop drivers, then perform the action
/// through the power device.
///
/// The power device is opened by `task` before anything is stopped, so the
/// final action does not depend on path resolution after the root filesystem
/// was unmounted, and an unsupported action fails before any work is done.
pub struct Supervisor<'a> {
    pub virtual_file_system: &'static VirtualFileSystem,
    pub time_manager: &'static time::Manager<'static>,
    /// Supervisor task; its descendants are stopped.
    pub task: TaskIdentifier,
    /// Runtime service subtrees left running (stopped by [`Drivers`]).
    pub services: &'a [TaskIdentifier],
    /// Quiescence budget on the monotonic clock.
    pub budget: Duration,
}

/// How long the supervisor retries opening a power device held by a request.
const OPEN_ATTEMPTS: usize = 50;
const OPEN_RETRY: Duration = Duration::from_millis(20);

impl Supervisor<'_> {
    /// Run the accepted request of `controller` once. Returns only on failure
    /// or when the final action returns.
    pub async fn run(
        &self,
        controller: &Controller,
        drivers: &mut impl Drivers,
    ) -> Result<(), Error> {
        let mut device = self.open_power_device().await;
        let capabilities = match device.as_mut() {
            Some(device) => device
                .control(GET_CAPABILITIES, &())
                .await
                .map(Capabilities)
                .unwrap_or_default(),
            None => Capabilities::NONE,
        };
        let mut stages = SystemStages {
            supervisor: self,
            device,
            capabilities,
            drivers,
        };
        let result = controller.run(&mut stages, self.budget).await;
        if let Some(device) = stages.device.take() {
            let _ = device.close(self.virtual_file_system).await;
        }
        result
    }

    async fn open_power_device(&self) -> Option<File<'static>> {
        for _ in 0..OPEN_ATTEMPTS {
            match File::open(
                self.virtual_file_system,
                self.task,
                POWER_DEVICE_PATH,
                AccessFlags::Write.into(),
            )
            .await
            {
                Ok(device) => return Some(device),
                // A concurrent privileged request briefly holds the device.
                Err(virtual_file_system::Error::FileSystem(file_system::Error::RessourceBusy))
                | Err(virtual_file_system::Error::RessourceBusy) => {
                    task::Manager::sleep(OPEN_RETRY).await
                }
                Err(_) => return None,
            }
        }
        None
    }
}

struct SystemStages<'a, 'b, D> {
    supervisor: &'a Supervisor<'b>,
    device: Option<File<'static>>,
    capabilities: Capabilities,
    drivers: &'a mut D,
}

impl<D: Drivers> Stages for SystemStages<'_, '_, D> {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    async fn quiesce(&mut self, budget: Duration) -> Result<(), Error> {
        let supervisor = self.supervisor;
        tasks::stop(
            supervisor.virtual_file_system.task_manager(),
            supervisor.task,
            supervisor.services,
            || supervisor.time_manager.monotonic_now(),
            budget,
        )
        .await
    }

    async fn storage(&mut self) -> Result<(), Error> {
        self.supervisor
            .virtual_file_system
            .shutdown_storage()
            .await
            .map_err(|_| Error::Storage)
    }

    async fn drivers(&mut self) -> Result<(), Error> {
        self.drivers.stop().await
    }

    async fn finalize(&mut self, action: Action, status: u8) -> Result<(), Error> {
        let device = self.device.as_mut().ok_or(Error::Unsupported)?;
        let result = match action {
            Action::PowerOff => device.control(POWER_OFF, &status).await,
            Action::Restart => device.control(RESTART, &status).await,
            Action::Sleep => device.control(SLEEP, &()).await,
        };
        result.map_err(|error| match error {
            virtual_file_system::Error::FileSystem(file_system::Error::UnsupportedOperation)
            | virtual_file_system::Error::UnsupportedOperation => Error::Unsupported,
            _ => Error::Power,
        })
    }
}
