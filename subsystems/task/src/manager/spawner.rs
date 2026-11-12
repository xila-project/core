use super::*;
use embassy_executor::{SendSpawner, Spawner};

pub type SpawnerIdentifier = usize;

pub(super) struct SpawnerState {
    pub executor: usize,
    /// Embassy's thread-safe enqueue handle, used only by `spawn_send`.
    pub send: SendSpawner,
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
                    send: spawner.make_send(),
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

    /// Select and reserve a registration in one critical section. `executor`
    /// restricts the choice to the caller's executor; non-Send futures must
    /// never be transferred to another executor thread. Without a request,
    /// the least-loaded registration wins. Unregistering a reserved
    /// registration fails until `release_spawner` or task exit.
    pub(super) fn reserve_spawner(
        &self,
        executor: Option<usize>,
        requested: Option<SpawnerIdentifier>,
    ) -> Result<(SpawnerIdentifier, SendSpawner)> {
        self.1.lock(|spawners| {
            let mut spawners = spawners.borrow_mut();
            let eligible = |state: &SpawnerState| executor.is_none_or(|e| state.executor == e);
            let (identifier, state) = match requested {
                Some(identifier) => spawners
                    .get_mut(&identifier)
                    .filter(|state| eligible(state))
                    .map(|state| (identifier, state))
                    .ok_or(Error::InvalidSpawnerIdentifier)?,
                None => spawners
                    .iter_mut()
                    .filter(|(_, state)| eligible(state))
                    .min_by_key(|(_, state)| state.tasks)
                    .map(|(identifier, state)| (*identifier, state))
                    .ok_or(Error::NoSpawnerAvailable)?,
            };
            state.tasks += 1;
            Ok((identifier, state.send))
        })
    }

    /// Undo a reservation whose task was never enqueued.
    pub(super) fn release_spawner(&self, identifier: SpawnerIdentifier) {
        self.1.lock(|spawners| {
            if let Some(state) = spawners.borrow_mut().get_mut(&identifier) {
                state.tasks -= 1;
            }
        });
    }

    pub async fn get_spawner(&self, task: TaskIdentifier) -> Result<SpawnerIdentifier> {
        Self::get_task(&*self.0.read().await, task).map(|task| task.spawner_identifier)
    }
}
