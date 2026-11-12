use super::*;
use embassy_executor::Spawner;

pub type SpawnerIdentifier = usize;

pub(super) struct SpawnerState {
    pub executor: usize,
    pub tasks: usize,
}

// Spawner is deliberately !Send. Never store it in the shared manager.
#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    target_arch = "wasm32"
))]
std::thread_local! {
    static LOCAL_SPAWNER: core::cell::Cell<Option<Spawner>> = const { core::cell::Cell::new(None) };
}

impl Manager {
    pub fn register_spawner(&'static self, spawner: Spawner) -> Result<SpawnerIdentifier> {
        let identifier = self.1.lock(|spawners| {
            let mut spawners = spawners.borrow_mut();
            let identifier = Self::find_first_available_identifier(&spawners, 0..usize::MAX)
                .ok_or(Error::TooManySpawners)?;
            spawners.insert(
                identifier,
                SpawnerState {
                    executor: spawner.executor_id(),
                    tasks: 0,
                },
            );
            Ok(identifier)
        })?;
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows",
            target_arch = "wasm32"
        ))]
        LOCAL_SPAWNER.set(Some(spawner));
        Ok(identifier)
    }

    /// Remove an idle registration; active registrations return `AlreadySet`.
    pub fn unregister_spawner(&'static self, identifier: SpawnerIdentifier) -> Result<()> {
        self.1.lock(|spawners| {
            let mut spawners = spawners.borrow_mut();
            let state = spawners.get(&identifier).ok_or(Error::NoSpawnerAvailable)?;
            if state.tasks != 0 {
                return Err(Error::AlreadySet);
            }
            spawners.remove(&identifier);
            Ok(())
        })
    }

    pub(super) async fn local_spawner() -> Result<Spawner> {
        // The checked waker guarantees the unsafe Embassy API's context contract.
        if Self::get_current_internal_identifier().await.is_ok() {
            return Ok(unsafe { Spawner::for_current_executor().await });
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows",
            target_arch = "wasm32"
        ))]
        if let Some(spawner) = LOCAL_SPAWNER.get() {
            return Ok(spawner);
        }
        Err(Error::ThreadNotRegistered)
    }

    /// Select only among registrations belonging to this executor. Non-Send
    /// futures must never be transferred to another executor thread.
    pub(super) fn select_best_spawner(
        &self,
        executor: usize,
        requested: Option<usize>,
    ) -> Result<usize> {
        self.1.lock(|spawners| {
            let spawners = spawners.borrow();
            if let Some(id) = requested {
                return spawners
                    .get(&id)
                    .filter(|state| state.executor == executor)
                    .map(|_| id)
                    .ok_or(Error::InvalidSpawnerIdentifier);
            }
            spawners
                .iter()
                .filter(|(_, state)| state.executor == executor)
                .min_by_key(|(_, state)| state.tasks)
                .map(|(id, _)| *id)
                .ok_or(Error::NoSpawnerAvailable)
        })
    }

    pub async fn get_spawner(&self, task: TaskIdentifier) -> Result<SpawnerIdentifier> {
        Self::get_task(&*self.0.read().await, task).map(|task| task.spawner_identifier)
    }
}
