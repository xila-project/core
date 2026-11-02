// Lifecycle module - handles task spawning, execution, and lifecycle management

use super::*;
use alloc::boxed::Box;
use core::{
    future::{Future, poll_fn},
    ptr::NonNull,
    task::Poll,
    time::Duration,
};
use embassy_executor::raw::{TaskPool, task_from_waker};
use embassy_time::Timer;

impl Manager {
    // Static function to create and execute tasks
    // This function is outside the closure that captures SpawnToken,
    // so it can be called safely from nested tasks
    async fn create_and_run_task<R: 'static, FunctionType, FutureType>(
        manager: &'static Manager,
        parent_task_identifier: TaskIdentifier,
        name: &str,
        function: FunctionType,
        spawner: Option<usize>,
    ) -> Result<(JoinHandle<R>, TaskIdentifier)>
    where
        FunctionType: FnOnce(TaskIdentifier) -> FutureType + 'static,
        FutureType: Future<Output = R> + 'static,
    {
        let identifier = manager
            .register(parent_task_identifier, name)
            .await
            .expect("Failed to get new task identifier");

        let pool = Box::new(TaskPool::<_, 1>::new());
        let pool = Box::leak(pool);

        let (join_handle_parent, join_handle_child) = JoinHandle::new();

        let mut inner = manager.0.write().await;

        // Select the best spawner for the new task
        let spawner = if let Some(spawner) = spawner {
            if !inner.spawners.contains_key(&spawner) {
                return Err(Error::InvalidSpawnerIdentifier);
            }
            spawner
        } else {
            Manager::select_best_spawner(&inner)?
        };

        inner
            .tasks
            .get_mut(&identifier)
            .expect("Failed to get task metadata")
            .spawner_identifier = spawner;

        #[cfg(all(feature = "profiling", target_os = "linux"))]
        let profiling_name = inner.tasks.get(&identifier).unwrap().name.clone();

        let task = async move || {
            let manager = manager;
            let internal_identifier = Manager::get_current_internal_identifier().await;

            manager
                .set_internal_identifier(identifier, internal_identifier)
                .await
                .expect("Failed to register task");

            #[cfg(all(feature = "profiling", target_os = "linux"))]
            let result = crate::profiling::instrument(
                manager as *const Manager as usize,
                identifier.into(),
                &profiling_name,
                spawner,
                function(identifier),
            )
            .await;
            #[cfg(not(all(feature = "profiling", target_os = "linux")))]
            let result = function(identifier).await;

            join_handle_child.signal(result);

            manager
                .unregister(identifier)
                .await
                .expect("Failed to unregister task");
        };

        let token = pool.spawn(task).expect("Failed to spawn task");

        inner
            .spawners
            .get(&spawner)
            .expect("Failed to get spawner")
            .spawn(token);

        Ok((join_handle_parent, identifier))
    }

    /// Spawn task
    pub async fn spawn<FunctionType, FutureType, ReturnType>(
        &'static self,
        parent_task: TaskIdentifier,
        name: &str,
        spawner: Option<usize>,
        function: FunctionType,
    ) -> Result<(JoinHandle<ReturnType>, TaskIdentifier)>
    where
        FunctionType: FnOnce(TaskIdentifier) -> FutureType + 'static,
        FutureType: Future<Output = ReturnType> + 'static,
        ReturnType: 'static,
    {
        // Call the helper function with all our parameters
        Self::create_and_run_task(self, parent_task, name, function, spawner).await
    }

    /// Set the internal identifier of a task.
    ///
    /// This function check if the task identifier is not already used,
    /// however it doesn't check if the parent task exists.
    async fn set_internal_identifier(
        &self,
        identifier: TaskIdentifier,
        internal_identifier: usize,
    ) -> Result<()> {
        let mut inner = self.0.write().await;

        let metadata = Self::get_task_mutable(&mut inner, identifier)?;

        metadata.internal_identifier = internal_identifier;

        // Register the internal identifier of the task
        if let Some(old_identifier) = inner.identifiers.insert(internal_identifier, identifier) {
            // Rollback the task registration if internal identifier registration fails
            inner.identifiers.remove(&internal_identifier);
            inner
                .identifiers
                .insert(internal_identifier, old_identifier);
            return Err(Error::InvalidTaskIdentifier);
        }

        Ok(())
    }

    /// Sleep the current thread for a given duration.
    pub async fn sleep(duration: Duration) {
        let nano_seconds = duration.as_nanos();

        Timer::after(embassy_time::Duration::from_nanos(nano_seconds as u64)).await
    }

    pub async fn get_current_internal_identifier() -> usize {
        poll_fn(|context| {
            let task_reference = task_from_waker(context.waker());

            let inner: NonNull<u8> = unsafe { core::mem::transmute(task_reference) };

            let identifier = inner.as_ptr() as usize;

            Poll::Ready(identifier)
        })
        .await
    }

    pub async fn get_current_task_identifier(&self) -> TaskIdentifier {
        let internal_identifier = Self::get_current_internal_identifier().await;

        {
            let inner = self.0.read().await;
            if let Some(identifier) = inner.identifiers.get(&internal_identifier) {
                return *identifier;
            }
        }

        // Every independently constructed task manager registers the executor's
        // current task as its own root task on first access.
        let mut inner = self.0.write().await;
        if let Some(identifier) = inner.identifiers.get(&internal_identifier) {
            return *identifier;
        }
        let generation = inner
            .next_generation
            .checked_add(1)
            .expect("task authority generations exhausted");
        inner.next_generation = generation;
        let root = Manager::ROOT_TASK_IDENTIFIER;
        inner.tasks.insert(
            root,
            Metadata {
                generation,
                internal_identifier,
                name: "Root".into(),
                parent: root,
                user: users::UserIdentifier::ROOT,
                group: users::GroupIdentifier::ROOT,
                environment_variables: alloc::vec::Vec::new(),
                signals: crate::signal::SignalAccumulator::new(),
                spawner_identifier: 0,
            },
        );
        inner.identifiers.insert(internal_identifier, root);

        root
    }
}
