use super::*;
use alloc::boxed::Box;
use core::{
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll, RawWakerVTable},
    time::Duration,
};
use embassy_executor::{
    SpawnToken, Spawner,
    raw::{AvailableTask, TaskStorage},
};
use embassy_time::Timer;

use portable_atomic::{AtomicPtr, Ordering};

static EMBASSY_WAKER_VTABLE: AtomicPtr<RawWakerVTable> = AtomicPtr::new(core::ptr::null_mut());

fn remember_vtable(vtable: &'static RawWakerVTable) {
    EMBASSY_WAKER_VTABLE.store(core::ptr::from_ref(vtable).cast_mut(), Ordering::Relaxed);
}

fn embassy_vtable() -> *const RawWakerVTable {
    EMBASSY_WAKER_VTABLE.load(Ordering::Relaxed)
}

/// Only TaskStorage polls this wrapper, so its context is an Embassy context.
/// Embassy 0.10 keeps try_task_from_waker private. Remember its vtable here
/// to reject foreign wakers before reading their opaque task identity.
/// This avoids relying on TaskRef's layout or private executor symbols.
pub(super) struct TaskFuture {
    future: Pin<Box<dyn Future<Output = ()>>>,
    #[cfg(feature = "metrics")]
    instrumentation: crate::metrics::TaskInstrumentation,
}

// SAFETY: Embassy needs `Send` only to enqueue a token through a `SendSpawner`;
// the storage itself is Sync whatever its future. `spawn_on` enqueues through
// the caller's !Send `Spawner`, so its futures never leave the executor thread.
// `spawn_send` futures cross threads exactly once, before their first poll,
// while their state is only the captures checked `Send` there. Embassy then
// creates the user future, polls it and drops it on the target executor only.
// This is the contract of Embassy's `_spawn_async_fn` for `#[task]` functions.
unsafe impl Send for TaskFuture {}

impl Future for TaskFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        remember_vtable(cx.waker().vtable());
        #[cfg(feature = "metrics")]
        {
            // Both times are read inside the executor's write sections, so
            // readers rarely extrapolate a finished poll; they clamp the rest.
            let instrumentation = self.instrumentation;
            let start = instrumentation.executor.begin_poll();
            let result = self.future.as_mut().poll(cx);
            let end = instrumentation.executor.end_period();
            instrumentation.task.record_poll(end.saturating_sub(start));
            result
        }
        #[cfg(not(feature = "metrics"))]
        self.future.as_mut().poll(cx)
    }
}

impl Manager {
    /// Spawn on the calling executor. An explicit foreign executor is rejected;
    /// use `spawn_send` to start work on another executor.
    /// Desktop and browser synchronous adapters may use the spawner registered on this thread.
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
        let local = Self::local_spawner().await?;
        self.spawn_on(local, parent_task, name, spawner, function)
            .await
    }

    /// Bootstrap or spawn using a caller-owned local spawner. Its !Send type
    /// enforces Embassy's thread affinity, including before the first poll.
    pub async fn spawn_on<FunctionType, FutureType, ReturnType>(
        &'static self,
        local: Spawner,
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
        let mut inner = self.0.write().await;
        let (selected, _) = self.reserve_spawner(Some(local.executor_id()), spawner)?;
        self.enqueue(&mut inner, selected, parent_task, name, function, |token| {
            local.spawn(token)
        })
    }

    /// Spawn a Send future on a specific registered executor.
    ///
    /// The task stays on that executor until it exits. An invalid identifier
    /// returns `InvalidSpawnerIdentifier`.
    ///
    /// ```compile_fail
    /// async fn non_send_future(manager: &'static task::Manager, executor: task::SpawnerIdentifier) {
    ///     let root = task::Manager::ROOT_TASK_IDENTIFIER;
    ///     manager.spawn_on_executor(root, "", executor, |_| async {
    ///         let local = std::rc::Rc::new(());
    ///         task::yield_now().await;
    ///         drop(local);
    ///     }).await.unwrap();
    /// }
    /// ```
    pub async fn spawn_on_executor<FunctionType, FutureType, ReturnType>(
        &'static self,
        parent_task: TaskIdentifier,
        name: &str,
        executor: SpawnerIdentifier,
        function: FunctionType,
    ) -> Result<(JoinHandle<ReturnType>, TaskIdentifier)>
    where
        FunctionType: FnOnce(TaskIdentifier) -> FutureType + Send + 'static,
        FutureType: Future<Output = ReturnType> + Send + 'static,
        ReturnType: Send + 'static,
    {
        self.spawn_send(parent_task, name, Some(executor), function)
            .await
    }

    /// Spawn on any registered executor: `spawner`, or the least-loaded one.
    /// Only the closure and the result cross threads. The closure runs on the
    /// target executor, so its future may be !Send, as with Embassy's tasks.
    /// ```
    /// async fn remote(manager: &'static task::Manager) {
    ///     let state = synchronization::Arc::new(());
    ///     let root = task::Manager::ROOT_TASK_IDENTIFIER;
    ///     let _ = manager.spawn_send(root, "", None, move |_| async move {
    ///         let local = std::rc::Rc::new(state);
    ///         task::yield_now().await;
    ///         drop(local)
    ///     });
    /// }
    /// ```
    /// ```compile_fail
    /// async fn capture(manager: &'static task::Manager) {
    ///     let state = std::rc::Rc::new(());
    ///     let root = task::Manager::ROOT_TASK_IDENTIFIER;
    ///     let _ = manager.spawn_send(root, "", None, move |_| async move { drop(state) });
    /// }
    /// ```
    /// ```compile_fail
    /// async fn output(manager: &'static task::Manager) {
    ///     let root = task::Manager::ROOT_TASK_IDENTIFIER;
    ///     let _ = manager.spawn_send(root, "", None, |_| async { std::rc::Rc::new(()) });
    /// }
    /// ```
    pub async fn spawn_send<FunctionType, FutureType, ReturnType>(
        &'static self,
        parent_task: TaskIdentifier,
        name: &str,
        spawner: Option<usize>,
        function: FunctionType,
    ) -> Result<(JoinHandle<ReturnType>, TaskIdentifier)>
    where
        FunctionType: FnOnce(TaskIdentifier) -> FutureType + Send + 'static,
        FutureType: Future<Output = ReturnType> + 'static,
        ReturnType: Send + 'static,
    {
        // `enqueue`'s future owns only these, plus Send identifiers and name,
        // until its first poll on the target executor (see TaskFuture's Send).
        fn sendable<T: Send>(_: &T) {}
        sendable(&self);
        sendable(&function);
        let mut inner = self.0.write().await;
        let (selected, remote) = self.reserve_spawner(None, spawner)?;
        self.enqueue(&mut inner, selected, parent_task, name, function, |token| {
            remote.spawn(token)
        })
    }

    /// Register, store and enqueue a task reserved on `selected`. No await
    /// occurs between reservation and enqueueing; any failure rolls back.
    fn enqueue<FunctionType, FutureType, ReturnType>(
        &'static self,
        inner: &mut Inner,
        selected: usize,
        parent_task: TaskIdentifier,
        name: &str,
        function: FunctionType,
        start: impl FnOnce(SpawnToken<TaskFuture>),
    ) -> Result<(JoinHandle<ReturnType>, TaskIdentifier)>
    where
        FunctionType: FnOnce(TaskIdentifier) -> FutureType + 'static,
        FutureType: Future<Output = ReturnType> + 'static,
        ReturnType: 'static,
    {
        let result = (|| {
            let identifier = Self::register_inner(inner, parent_task, name)?;
            #[cfg(all(feature = "profiling", target_os = "linux"))]
            let profiling_name = inner.tasks[&identifier].name.clone();
            let (parent, child) = JoinHandle::new();
            #[cfg(feature = "metrics")]
            let executor = self.spawner_counters(selected);
            let mut future = Some(move || -> Pin<Box<dyn Future<Output = ()>>> {
                Box::pin(async move {
                    #[cfg(all(feature = "profiling", target_os = "linux"))]
                    let result = crate::profiling::instrument(
                        self as *const Manager as usize,
                        identifier.into(),
                        &profiling_name,
                        selected,
                        function(identifier),
                    )
                    .await;
                    #[cfg(not(all(feature = "profiling", target_os = "linux")))]
                    let result = function(identifier).await;
                    // Complete metadata cleanup before publishing the result.
                    let _ = self.unregister(identifier).await;
                    child.signal(result);
                })
            });
            // TaskStorage is retained forever: stale wakers and queued exited
            // tasks can still touch its header. Only the boxed future is freed
            // by Embassy's drop_in_place on Poll::Ready. Claiming fails while
            // Embassy still owns a slot, so reuse is executor-safe, including
            // on another executor, which Embassy explicitly allows.
            loop {
                for (index, slot) in inner.slots.iter().enumerate() {
                    if let Some(available) = AvailableTask::claim(slot) {
                        let future = future.take().unwrap();
                        #[cfg(feature = "metrics")]
                        let instrumentation = crate::metrics::TaskInstrumentation {
                            task: inner.counters[index],
                            executor,
                        };
                        // The claim orders this reset after the previous
                        // task's last poll, so the slot has one writer.
                        #[cfg(feature = "metrics")]
                        instrumentation.task.reset();
                        #[cfg(not(feature = "metrics"))]
                        let _ = index;
                        let token = available.initialize(move || TaskFuture {
                            future: future(),
                            #[cfg(feature = "metrics")]
                            instrumentation,
                        });
                        // Embassy's id() truncates pointers to u32 on 64-bit hosts.
                        // Use the full public waker data identity instead; TaskStorage
                        // is repr(C), with its header at offset zero.
                        let internal = core::ptr::from_ref(*slot) as usize;
                        let metadata = inner.tasks.get_mut(&identifier).unwrap();
                        metadata.internal_identifier = internal;
                        metadata.spawner_identifier = selected;
                        #[cfg(feature = "metrics")]
                        {
                            metadata.instrumentation = Some(instrumentation);
                        }
                        inner.identifiers.insert(internal, identifier);
                        start(token);
                        return Ok((parent, identifier));
                    }
                }
                if inner.slots.len() >= inner.maximum_tasks {
                    inner.tasks.remove(&identifier);
                    return Err(Error::TooManyTasks);
                }
                // Two slots per allocation, capped exactly at the configured limit.
                let first = inner.slots.len();
                let count = (inner.maximum_tasks - first).min(2);
                if inner.static_slots.is_empty() {
                    let chunk = (0..count)
                        .map(|_| TaskStorage::<TaskFuture>::new())
                        .collect::<alloc::vec::Vec<_>>()
                        .into_boxed_slice();
                    inner.slots.extend(Box::leak(chunk).iter());
                } else {
                    inner.slots.extend(
                        inner.static_slots[first..first + count]
                            .iter()
                            .map(|slot| &slot.0),
                    );
                }
                #[cfg(feature = "metrics")]
                {
                    let counters = (0..count)
                        .map(|_| crate::metrics::TaskCounters::new())
                        .collect::<alloc::vec::Vec<_>>()
                        .into_boxed_slice();
                    inner.counters.extend(Box::leak(counters).iter());
                }
            }
        })();
        if result.is_err() {
            self.release_spawner(selected);
        }
        result
    }

    pub async fn sleep(duration: Duration) {
        let nanoseconds = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        Timer::at(deadline_after_nanoseconds(nanoseconds)).await
    }

    /// Return an error under block_on or any executor with a foreign waker.
    pub async fn get_current_internal_identifier() -> Result<usize> {
        poll_fn(|cx| {
            let expected = embassy_vtable();
            Poll::Ready(if core::ptr::eq(cx.waker().vtable(), expected) {
                Ok(cx.waker().data() as usize)
            } else {
                Err(Error::ThreadNotRegistered)
            })
        })
        .await
    }

    pub async fn try_get_current_task_identifier(&self) -> Result<TaskIdentifier> {
        let internal = Self::get_current_internal_identifier().await?;
        let mut inner = self.0.write().await;
        if let Some(identifier) = inner.identifiers.get(&internal) {
            return Ok(*identifier);
        }
        // A new independent manager may adopt its first Embassy task as root.
        // Once established, an unknown task can never replace it.
        if inner.root_established {
            return Err(Error::ThreadNotRegistered);
        }
        let root = Self::register_inner(&mut inner, Self::ROOT_TASK_IDENTIFIER, "Root")?;
        inner.tasks.get_mut(&root).unwrap().internal_identifier = internal;
        inner.identifiers.insert(internal, root);
        Ok(root)
    }

    /// Compatibility API. Unknown tasks fail closed; use the fallible variant
    /// when callers need to handle missing registration.
    pub async fn get_current_task_identifier(&self) -> TaskIdentifier {
        self.try_get_current_task_identifier()
            .await
            .expect("task is not registered")
    }
}
