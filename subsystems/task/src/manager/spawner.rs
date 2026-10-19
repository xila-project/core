// Spawner module - handles spawner registration and selection

use super::*;

use alloc::collections::BTreeMap;
use embassy_executor::Spawner;

pub type SpawnerIdentifier = usize;

impl Manager {
    pub fn register_spawner(&'static self, spawner: Spawner) -> Result<SpawnerIdentifier> {
        let mut inner = embassy_futures::block_on(self.0.write());

        let identifier = Self::find_first_available_identifier(
            &inner.spawners,
            (usize::MIN..usize::MAX).step_by(1),
        )
        .ok_or(Error::TooManySpawners)?;

        if inner.spawners.insert(identifier, spawner).is_some() {
            unreachable!("Spawner identifier already exists");
        }

        Ok(identifier)
    }

    pub fn unregister_spawner(&'static self, identifier: SpawnerIdentifier) -> Result<()> {
        let mut inner = embassy_futures::block_on(self.0.write());

        inner
            .spawners
            .remove(&identifier)
            .ok_or(Error::NoSpawnerAvailable)?;

        Ok(())
    }

    /// Select the best spawner for a new task using load balancing algorithm
    pub(crate) fn select_best_spawner(inner: &Inner) -> Result<SpawnerIdentifier> {
        if inner.spawners.is_empty() {
            return Err(Error::NoSpawnerAvailable);
        }

        let mut map = BTreeMap::new();

        for identifier in inner.spawners.keys() {
            map.insert(*identifier, 0); // Initialize all spawners with a load of 0
        }

        for metadata in inner.tasks.values() {
            if let Some(load) = map.get_mut(&metadata.spawner_identifier) {
                *load += 1; // Increment the load for the spawner
            }
        }

        // Find the spawner with the lowest load score
        let mut best_index = 0;
        let mut best_score = usize::MAX;

        for (identifier, spawner) in map.iter() {
            if *spawner < best_score {
                best_score = *spawner;
                best_index = *identifier;
            }
        }

        Ok(best_index)
    }

    pub async fn get_spawner(&self, task: TaskIdentifier) -> Result<SpawnerIdentifier> {
        Self::get_task(&*self.0.read().await, task)
            .map(|task| task.spawner_identifier)
            .map_err(|_| Error::InvalidTaskIdentifier)
    }
}
