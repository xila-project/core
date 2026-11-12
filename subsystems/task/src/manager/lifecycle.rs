use super::*;
use alloc::boxed::Box;
use core::{
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll, RawWakerVTable},
    time::Duration,
};
use embassy_executor::{Spawner, raw::TaskStorage};
use embassy_time::Timer;

// Use the same critical-section primitive on targets without pointer atomics.
#[cfg(target_has_atomic = "ptr")]
static EMBASSY_WAKER_VTABLE: core::sync::atomic::AtomicPtr<RawWakerVTable> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
#[cfg(not(target_has_atomic = "ptr"))]
static EMBASSY_WAKER_VTABLE: synchronization::blocking_mutex::Mutex<
    CriticalSectionRawMutex,
    core::cell::Cell<Option<&'static RawWakerVTable>>,
> = synchronization::blocking_mutex::Mutex::new(core::cell::Cell::new(None));

fn remember_vtable(vtable: &'static RawWakerVTable) {
    #[cfg(target_has_atomic = "ptr")]
    EMBASSY_WAKER_VTABLE.store(
        core::ptr::from_ref(vtable).cast_mut(),
        core::sync::atomic::Ordering::Relaxed,
    );
    #[cfg(not(target_has_atomic = "ptr"))]
    EMBASSY_WAKER_VTABLE.lock(|cell| cell.set(Some(vtable)));
}

fn embassy_vtable() -> *const RawWakerVTable {
    #[cfg(target_has_atomic = "ptr")]
    {
        EMBASSY_WAKER_VTABLE.load(core::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(target_has_atomic = "ptr"))]
    {
        EMBASSY_WAKER_VTABLE.lock(|cell| cell.get().map_or(core::ptr::null(), core::ptr::from_ref))
    }
}

/// Only TaskStorage polls this wrapper, so its context is an Embassy context.
/// Embassy 0.10 keeps try_task_from_waker private. Remember its vtable here
/// to reject foreign wakers before reading their opaque task identity.
/// This avoids relying on TaskRef's layout or private executor symbols.
pub(super) struct TaskFuture(Pin<Box<dyn Future<Output = ()>>>);
impl Future for TaskFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        remember_vtable(cx.waker().vtable());
        self.0.as_mut().poll(cx)
    }
}

impl Manager {
    /// Spawn on the calling executor. An explicit foreign executor is rejected.
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
        // Reserve the spawner while holding its short synchronous lock. No
        // await occurs between reservation and enqueueing; unregister rejects
        // a registration with outstanding tasks.
        let selected = self.select_best_spawner(local.executor_id(), spawner)?;
        self.1.lock(|spawners| {
            let mut spawners = spawners.borrow_mut();
            let state = spawners
                .get_mut(&selected)
                .ok_or(Error::NoSpawnerAvailable)?;
            if state.executor != local.executor_id() {
                return Err(Error::InvalidSpawnerIdentifier);
            }
            state.tasks += 1;
            Ok(())
        })?;
        let result = (|| {
            let identifier = Self::register_inner(&mut inner, parent_task, name)?;
            #[cfg(all(feature = "profiling", target_os = "linux"))]
            let profiling_name = inner.tasks[&identifier].name.clone();
            let (parent, child) = JoinHandle::new();
            let mut future = Some(TaskFuture(Box::pin(async move {
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
            })));
            // TaskStorage is retained forever: stale wakers and queued exited
            // tasks can still touch its header. Only the boxed future is freed
            // by Embassy's drop_in_place on Poll::Ready. spawn's Busy check
            // includes Embassy's queued state, so reuse is executor-safe.
            loop {
                for slot in &inner.slots {
                    if let Ok(token) = slot.spawn(|| future.take().unwrap()) {
                        // Embassy's id() truncates pointers to u32 on 64-bit hosts.
                        // Use the full public waker data identity instead; TaskStorage
                        // is repr(C), with its header at offset zero.
                        let internal = core::ptr::from_ref(*slot) as usize;
                        let metadata = inner.tasks.get_mut(&identifier).unwrap();
                        metadata.internal_identifier = internal;
                        metadata.spawner_identifier = selected;
                        inner.identifiers.insert(internal, identifier);
                        local.spawn(token);
                        return Ok((parent, identifier));
                    }
                }
                if inner.slots.len() >= inner.maximum_tasks {
                    inner.tasks.remove(&identifier);
                    return Err(Error::TooManyTasks);
                }
                // Two slots per allocation, capped exactly at the configured limit.
                let count = (inner.maximum_tasks - inner.slots.len()).min(2);
                let chunk = (0..count)
                    .map(|_| TaskStorage::<TaskFuture>::new())
                    .collect::<alloc::vec::Vec<_>>()
                    .into_boxed_slice();
                inner.slots.extend(Box::leak(chunk).iter());
            }
        })();
        if result.is_err() {
            self.1.lock(|spawners| {
                spawners.borrow_mut().get_mut(&selected).unwrap().tasks -= 1;
            });
        }
        result
    }

    pub async fn sleep(duration: Duration) {
        Timer::after(embassy_time::Duration::from_nanos(
            duration.as_nanos() as u64
        ))
        .await
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
