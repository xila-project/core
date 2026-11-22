use super::*;
use crate::{Process, Status, registry::Registry, suspend_dynamic};
use alloc::{alloc::alloc, boxed::Box, vec::Vec};
use core::{
    alloc::Layout,
    cell::{Cell, RefCell, UnsafeCell},
    future::poll_fn,
    pin::pin,
    ptr::{self, NonNull},
    task::{Poll, Waker},
};
use stackful::{Coroutine, Stack};

mod sync;
pub use sync::*;

const MAIN_THREAD: usize = 1;

struct Local<'group> {
    group: &'group Group,
    id: usize,
    libc: UnsafeCell<ThreadLibc>,
    values: [Cell<(usize, *mut c_void)>; XILA_THREAD_KEYS_MAX],
}

impl<'group> Local<'group> {
    const fn new(group: &'group Group, id: usize) -> Self {
        Self {
            group,
            id,
            libc: UnsafeCell::new(ThreadLibc {
                error: 0,
                strtok_next: ptr::null_mut(),
                random_state: 1,
            }),
            values: [const { Cell::new((0, ptr::null_mut())) }; XILA_THREAD_KEYS_MAX],
        }
    }

    /// Allocate a worker's `Local` on the heap, or fail with `OutOfMemory`.
    fn try_boxed(group: &'group Group, id: usize) -> Result<Box<Self>, Status> {
        let layout = Layout::new::<Self>();
        // SAFETY: `Local` is not zero-sized.
        let memory =
            NonNull::new(unsafe { alloc(layout) }.cast::<Self>()).ok_or(Status::OutOfMemory)?;
        // SAFETY: the global allocator returned this block for the layout of `Local`, which
        // `Box` frees with the same layout.
        unsafe {
            memory.write(Self::new(group, id));
            Ok(Box::from_raw(memory.as_ptr()))
        }
    }
}

struct Worker {
    id: usize,
    creator: usize,
    coroutine: Option<Coroutine>,
    /// The worker's identifier, TLS values and libc storage, which its native context points
    /// to. They live in this record, on the heap, and never on the worker's own coroutine
    /// stack: an overflow there must not reach what `xila_process_exit` relies on (#778).
    /// Boxed so that its address, which `xila_thread_libc` hands out, survives the
    /// reallocation of the worker list. Declared after `coroutine`, so a suspended
    /// coroutine is discarded before it.
    local: Box<Local<'static>>,
    result: Option<*mut c_void>,
    detached: bool,
    joining: bool,
}

struct Group {
    process: Cell<*const Process>,
    /// The handles the process opened: the host state's registry.
    registry: Cell<*const Registry>,
    workers: RefCell<Vec<Option<Worker>>>,
    /// Main's coroutine, out of here while it runs. Declared after `workers`, so a
    /// cancellation drops the workers first, as `shut_down` stops them first.
    main: RefCell<Option<Coroutine>>,
    next_thread: Cell<usize>,
    keys: [Cell<usize>; XILA_THREAD_KEYS_MAX],
    next_key: Cell<usize>,
    repoll: Cell<bool>,
    shutdown_thread: Cell<usize>,
}

impl Group {
    const fn new() -> Self {
        Self {
            process: Cell::new(ptr::null()),
            registry: Cell::new(ptr::null()),
            workers: RefCell::new(Vec::new()),
            main: RefCell::new(None),
            next_thread: Cell::new(MAIN_THREAD + 1),
            keys: [const { Cell::new(0) }; XILA_THREAD_KEYS_MAX],
            next_key: Cell::new(XILA_THREAD_KEYS_MAX),
            repoll: Cell::new(false),
            shutdown_thread: Cell::new(0),
        }
    }

    fn notify(&self) {
        self.repoll.set(true);
    }

    fn with_worker<R>(
        &self,
        id: usize,
        f: impl FnOnce(&mut Option<Worker>) -> Result<R, Status>,
    ) -> Result<R, Status> {
        let mut workers = self.workers.borrow_mut();
        let slot = workers
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|worker| worker.id == id))
            .ok_or(Status::NoSuchTask)?;
        f(slot)
    }

    /// Only one executor poll drives the group. Use its real waker for each
    /// child future, including Embassy timers which require an Embassy waker.
    /// A wake polls the group again. No future wakes just because it is pending.
    fn poll_workers(&self, waker: &Waker) -> Poll<i32> {
        let count = self.workers.borrow().len();
        for index in 0..count {
            // Release the borrow before resuming arbitrary C (which can spawn).
            let shutdown_thread = self.shutdown_thread.get();
            let Some((mut coroutine, local)) = self.workers.borrow_mut()[index]
                .as_mut()
                .filter(|worker| shutdown_thread == 0 || shutdown_thread == worker.id)
                .and_then(|worker| {
                    let local = ptr::from_ref::<Local<'static>>(&worker.local);
                    worker.coroutine.take().map(|coroutine| (coroutine, local))
                })
            else {
                continue;
            };
            // The record, and so its boxed Local, stays in the list until the worker has
            // stored its result, which is the last thing it does before it finishes.
            unsafe { coroutine.set_native_context(local.cast()) };
            let result = coroutine.resume(&mut core::task::Context::from_waker(waker));
            unsafe { coroutine.set_native_context(ptr::null()) };
            let mut workers = self.workers.borrow_mut();
            let slot = &mut workers[index];
            if let Some(worker) = slot {
                if result.is_pending() {
                    worker.coroutine = Some(coroutine);
                    continue;
                }
                let id = worker.id;
                if self.shutdown_thread.get() == id {
                    return result;
                }
                if worker.detached {
                    *slot = None;
                }
                // A finished creator can never join its children. Detach them,
                // and reap any that have already returned.
                for slot in &mut *workers {
                    if let Some(child) = slot
                        && child.creator == id
                    {
                        child.detached = true;
                        if child.result.is_some() {
                            *slot = None;
                        }
                    }
                }
                self.notify();
            }
        }
        Poll::Pending
    }

    /// Begin terminal cleanup on behalf of `caller`: from now on only it runs, and every
    /// other thread, main included, is stopped (see [`Coroutine::stop`]). A stopped thread's
    /// cancellable wait is dropped here, so whatever it held, such as the VFS namespace
    /// lock a path call holds while it waits, is free before the caller's cleanup needs
    /// it. The stacks stay until the driver drops the coroutines, because the caller may
    /// still read them (a stream buffer on main's stack, say).
    fn shut_down(&self, caller: usize) {
        self.shutdown_thread.set(caller);
        let count = self.workers.borrow().len();
        for index in 0..count {
            // Taken out, so that a destructor that suspends does not find the list
            // borrowed, then put back for the driver to drop.
            let taken = self.workers.borrow_mut()[index]
                .as_mut()
                .and_then(|worker| worker.coroutine.take());
            if let Some(mut coroutine) = taken {
                coroutine.stop();
                if let Some(worker) = self.workers.borrow_mut()[index].as_mut() {
                    worker.coroutine = Some(coroutine);
                }
            }
        }
        // Main's coroutine is out of the group while main runs, so main never stops itself.
        let main = self.main.borrow_mut().take();
        if let Some(mut main) = main {
            main.stop();
            *self.main.borrow_mut() = Some(main);
        }
    }

    fn key(&self, key: usize) -> Result<&Cell<usize>, Status> {
        let slot = &self.keys[key % XILA_THREAD_KEYS_MAX];
        if key == 0 || slot.get() != key {
            return Err(Status::InvalidArgument);
        }
        Ok(slot)
    }
}

fn current() -> Result<(&'static Group, &'static Local<'static>), Status> {
    // The pinned driver owns the Group for the lifetime of every runnable
    // coroutine. The driver installs the Local of the coroutine it resumes around
    // that resume: main's in the driver future, a worker's in its record. Neither
    // is on a coroutine stack. The erased lifetime lets ABI calls suspend, but
    // references must never escape this group.
    let local = unsafe { stackful::native_context().cast::<Local<'static>>().as_ref() }
        .ok_or(Status::Unsupported)?;
    Ok((local.group, local))
}

pub(crate) fn bind_process(process: *const Process) {
    if let Ok((group, _)) = current() {
        group.process.set(process);
    }
}

/// Record where the calling process's handles are registered, for every thread of it.
pub(crate) fn bind_registry(registry: *const Registry) {
    if let Ok((group, _)) = current() {
        group.registry.set(registry);
    }
}

/// The registry of the calling thread's process, once its entry has started.
///
/// # Safety
/// The reference must not be used after the process's host state is released.
pub(crate) unsafe fn registry<'a>() -> Option<&'a Registry> {
    let (group, _) = current().ok()?;
    // SAFETY: the host state outlives every coroutine of the process, as the caller assures.
    unsafe { group.registry.get().as_ref() }
}

pub(crate) fn stop_workers() {
    if let Ok((group, local)) = current() {
        group.shut_down(local.id);
    }
}

/// Begin terminal process cleanup. Only the calling thread may run after
/// this call, even if cleanup suspends. Spawning new workers is disabled.
///
/// The other threads are stopped at once: a cancellable wait one of them is suspended
/// in (a path call holding the VFS namespace lock, a socket being connected) is dropped
/// now, so cleanup never waits for what it held. Their stacks stay until the process ends.
///
/// A stopped thread that was waiting on a mutex or a condition variable stays in its
/// queue. Terminal cleanup must therefore only try-lock (`XILA_MUTEX_TRY`): an unlock that
/// finds a queued waiter hands the mutex to a thread that never runs again, and a blocking
/// lock behind a stopped owner or waiter never returns, so exit would hang.
/// # Safety
/// The caller must complete process exit without waiting for stopped peers.
/// Stopped coroutines and their pending operations are never resumed.
pub unsafe extern "C" fn xila_thread_stop_others(process: *const Process) -> Status {
    status((|| {
        let (group, local) = current()?;
        if process.is_null() || group.process.get() != process {
            return Err(Status::InvalidArgument);
        }
        group.shut_down(local.id);
        Ok(())
    })())
}

/// Finish the calling worker with a pointer-sized result. Main must use
/// process exit or return. The worker's native frames are abandoned.
/// # Safety
/// The caller must release resources that would otherwise be leaked when its
/// frames are abandoned. The result is opaque and must survive worker exit.
pub unsafe extern "C" fn xila_thread_exit(result: *mut c_void) -> Status {
    status((|| {
        let (group, local) = current()?;
        if local.id == MAIN_THREAD || group.shutdown_thread.get() != 0 {
            return Err(Status::InvalidState);
        }
        group.with_worker(local.id, |slot| {
            slot.as_mut().ok_or(Status::NoSuchTask)?.result = Some(result);
            Ok(())
        })?;
        crate::exit_coroutine(0);
    })())
}

pub(crate) fn prepare_exit(process: *const Process) -> Result<(), Status> {
    if let Ok((group, local)) = current() {
        if group.process.get() != process {
            return Err(Status::InvalidArgument);
        }
        group.shut_down(local.id);
    }
    Ok(())
}

/// Drive the process entry and all its workers on the same executor task.
/// The group and its workers are dropped before this future returns. A suspended
/// coroutine, main or worker, is cancelled: its frames are discarded and its stack freed.
/// What the process owns lives in the host state, which the caller releases afterwards.
pub async fn drive_process(main: Coroutine) -> i32 {
    let group = Group::new();
    *group.main.borrow_mut() = Some(main);
    let local = Local::new(&group, MAIN_THREAD);
    poll_fn(|cx| {
        group.repoll.set(false);

        match group.shutdown_thread.get() {
            0 | MAIN_THREAD => {
                // Taken out while it runs, like a worker's, and put back if it suspends.
                let main = group.main.borrow_mut().take();
                if let Some(mut main) = main {
                    // Both locals are borrowed by this pinned future.
                    unsafe { main.set_native_context(ptr::from_ref(&local).cast()) };
                    let result = main.resume(cx);
                    unsafe { main.set_native_context(ptr::null()) };
                    if result.is_ready() {
                        return result;
                    }
                    *group.main.borrow_mut() = Some(main);
                }
            }
            _ => {}
        }
        if group.shutdown_thread.get() != MAIN_THREAD {
            let result = group.poll_workers(cx.waker());
            if result.is_ready() {
                return result;
            }
        }
        if group.repoll.get() {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
}

fn status(result: Result<(), Status>) -> Status {
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Start a coroutine on the current process's executor. Zero stack size uses
/// 4 KiB on bare metal or 64 KiB on Linux.
/// # Safety
/// The process and output must be live. The entry must never unwind or exceed
/// the requested stack and the argument must remain valid until entry ends.
pub unsafe extern "C" fn xila_thread_spawn(
    process: *const Process,
    stack_size: usize,
    entry: ThreadEntry,
    argument: *mut c_void,
    thread: *mut usize,
) -> Status {
    status((|| {
        let (group, local) = current()?;
        let entry = entry.ok_or(Status::InvalidArgument)?;
        if process.is_null() || group.process.get() != process || thread.is_null() {
            return Err(Status::InvalidArgument);
        }
        if group.shutdown_thread.get() != 0 {
            return Err(Status::InvalidState);
        }
        let stack_size = match stack_size {
            0 if cfg!(target_os = "linux") => XILA_THREAD_STACK_MAX,
            0 => stackful::DEFAULT_MIN_STACK_SIZE,
            XILA_THREAD_STACK_MIN..=XILA_THREAD_STACK_MAX => stack_size,
            _ => return Err(Status::InvalidArgument),
        };

        let id = group.next_thread.get();
        let next = id.checked_add(1).ok_or(Status::Overflow)?;
        let stack = Stack::try_with_min_size(stack_size, XILA_THREAD_STACK_MIN)
            .ok_or(Status::OutOfMemory)?;
        let worker_local = Local::try_boxed(group, id)?;
        let mut workers = group.workers.borrow_mut();
        let index = if let Some(index) = workers.iter().position(Option::is_none) {
            index
        } else {
            workers.try_reserve(1).map_err(|_| Status::OutOfMemory)?;
            workers.push(None);
            workers.len() - 1
        };
        // The entry captures only what the program needs to start. The driver installs the
        // worker's Local, from its record, around each resume.
        let coroutine = unsafe {
            Coroutine::new(stack, move || {
                let result = entry(argument);
                if let Ok((group, local)) = current() {
                    let _ = group.with_worker(local.id, |slot| {
                        slot.as_mut().ok_or(Status::NoSuchTask)?.result = Some(result);
                        Ok(())
                    });
                }
                0
            })
        };
        workers[index] = Some(Worker {
            id,
            creator: local.id,
            coroutine: Some(coroutine),
            local: worker_local,
            result: None,
            detached: false,
            joining: false,
        });
        group.next_thread.set(next);
        group.notify();
        unsafe { thread.write(id) };
        Ok(())
    })())
}

/// Wait for one worker, then reap its result. Joining oneself fails with Deadlock.
/// # Safety
/// A non-null result must be writable after the wait.
pub unsafe extern "C" fn xila_thread_join(thread: usize, result: *mut *mut c_void) -> Status {
    status((|| {
        let (group, local) = current()?;
        if thread == local.id {
            return Err(Status::Deadlock);
        }
        group.with_worker(thread, |slot| {
            let worker = slot.as_mut().ok_or(Status::NoSuchTask)?;
            if worker.creator != local.id {
                return Err(Status::NoChild);
            }
            if worker.detached || worker.joining {
                return Err(Status::InvalidArgument);
            }
            worker.joining = true;
            Ok(())
        })?;
        let mut value = Err(Status::NoSuchTask);
        suspend_dynamic(pin!(poll_fn(|_| {
            let response = group.with_worker(thread, |slot| {
                let result = slot.as_ref().and_then(|worker| worker.result);
                if result.is_some() {
                    *slot = None;
                }
                Ok(result)
            });
            if matches!(response, Ok(None)) {
                Poll::Pending
            } else {
                value = response.and_then(|value| value.ok_or(Status::NoSuchTask));
                Poll::Ready(())
            }
        })));
        let value = value?;
        if !result.is_null() {
            unsafe { result.write(value) };
        }
        Ok(())
    })())
}

/// Detach a worker. Its result is discarded, and it is reaped on return.
pub extern "C" fn xila_thread_detach(thread: usize) -> Status {
    status((|| {
        let (group, _) = current()?;
        group.with_worker(thread, |slot| {
            let worker = slot.as_mut().ok_or(Status::NoSuchTask)?;
            if worker.detached || worker.joining {
                return Err(Status::InvalidArgument);
            }
            worker.detached = true;
            if worker.result.is_some() {
                *slot = None;
            }
            Ok(())
        })
    })())
}

/// Yield once to the other coroutines on the executor.
pub extern "C" fn xila_thread_yield() -> Status {
    if let Err(error) = current() {
        return error;
    }
    crate::yield_once();
    Status::Ok
}

/// Read the calling thread's process-local identifier.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_thread_current(thread: *mut usize) -> Status {
    status((|| {
        let (_, local) = current()?;
        if thread.is_null() {
            return Err(Status::InvalidArgument);
        }
        unsafe { thread.write(local.id) };
        Ok(())
    })())
}

/// Report whether the process still has live workers. Finished workers whose
/// results have not been joined do not count.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_thread_has_workers(live: *mut bool) -> Status {
    status((|| {
        let (group, _) = current()?;
        if live.is_null() {
            return Err(Status::InvalidArgument);
        }
        let workers = group.workers.borrow();
        let has_workers = workers
            .iter()
            .flatten()
            .any(|worker| worker.result.is_none());
        unsafe { live.write(has_workers) };
        Ok(())
    })())
}

/// Allocate a process-local TLS key. New keys read as null in all threads.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_thread_key_create(key: *mut usize) -> Status {
    status((|| {
        let (group, _) = current()?;
        if key.is_null() {
            return Err(Status::InvalidArgument);
        }
        let (index, slot) = group
            .keys
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.get() == 0)
            .ok_or(Status::OutOfMemory)?;
        let base = group.next_key.get();
        group.next_key.set(
            base.checked_add(XILA_THREAD_KEYS_MAX)
                .ok_or(Status::Overflow)?,
        );
        let value = base + index;
        slot.set(value);
        unsafe { key.write(value) };
        Ok(())
    })())
}

/// Invalidate a process-local TLS key without calling destructors.
pub extern "C" fn xila_thread_key_delete(key: usize) -> Status {
    status((|| {
        let (group, _) = current()?;
        group.key(key)?.set(0);
        Ok(())
    })())
}

/// Read a TLS value from the current coroutine.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_thread_tls_get(key: usize, value: *mut *mut c_void) -> Status {
    status((|| {
        let (group, local) = current()?;
        group.key(key)?;
        if value.is_null() {
            return Err(Status::InvalidArgument);
        }
        let (stored_key, stored_value) = local.values[key % XILA_THREAD_KEYS_MAX].get();
        unsafe {
            value.write(if stored_key == key {
                stored_value
            } else {
                ptr::null_mut()
            })
        };
        Ok(())
    })())
}

/// Write a pointer-sized value in the current coroutine. The host never
/// dereferences or frees it.
pub extern "C" fn xila_thread_tls_set(key: usize, value: *mut c_void) -> Status {
    status((|| {
        let (group, local) = current()?;
        group.key(key)?;
        local.values[key % XILA_THREAD_KEYS_MAX].set((key, value));
        Ok(())
    })())
}

/// Read the address of the current coroutine's libc storage.
/// # Safety
/// Output must be aligned and writable. The returned pointer is borrowed until
/// the calling coroutine ends and must only be used by that coroutine.
pub unsafe extern "C" fn xila_thread_libc(location: *mut *mut ThreadLibc) -> Status {
    status((|| {
        let (_, local) = current()?;
        if location.is_null() {
            return Err(Status::InvalidArgument);
        }
        unsafe { location.write(local.libc.get()) };
        Ok(())
    })())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use core::{future::Future, task::Context};

    #[test]
    fn coroutine_tls_survives_yield_and_key_reuse_starts_empty() {
        let mut id = 99;
        assert_eq!(unsafe { xila_thread_current(&mut id) }, Status::Unsupported);
        assert_eq!(id, 99);

        let main = unsafe {
            Coroutine::new(Stack::new(64 * 1024), || {
                let mut keys = [0; XILA_THREAD_KEYS_MAX];
                for key in &mut keys {
                    assert_eq!(xila_thread_key_create(key), Status::Ok);
                }
                let mut extra = 0;
                assert_eq!(xila_thread_key_create(&mut extra), Status::OutOfMemory);
                assert_eq!(extra, 0);

                let value = ptr::dangling_mut();
                assert_eq!(xila_thread_tls_set(keys[0], value), Status::Ok);
                assert_eq!(xila_thread_yield(), Status::Ok);
                let mut stored = ptr::null_mut();
                assert_eq!(xila_thread_tls_get(keys[0], &mut stored), Status::Ok);
                assert_eq!(stored, value);

                assert_eq!(xila_thread_key_delete(keys[0]), Status::Ok);
                assert_eq!(xila_thread_key_create(&mut extra), Status::Ok);
                assert_ne!(extra, keys[0]);
                assert_eq!(xila_thread_tls_get(extra, &mut stored), Status::Ok);
                assert!(stored.is_null());
                17
            })
        };
        let mut driver = pin!(drive_process(main));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(driver.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(driver.as_mut().poll(&mut cx), Poll::Ready(17));
    }

    // Keep the probes bounded so a missed wake is a test failure.
    fn run_probe(main: impl FnOnce() -> i32 + 'static) -> i32 {
        let main = unsafe { Coroutine::new(Stack::new(64 * 1024), main) };
        let mut driver = pin!(drive_process(main));
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..200 {
            if let Poll::Ready(status) = driver.as_mut().poll(&mut cx) {
                return status;
            }
        }
        panic!("process did not finish");
    }

    struct BroadcastProbe {
        mutex: Mutex,
        condition: Condition,
        woken: usize,
    }

    unsafe extern "C" fn probe_waiter(argument: *mut c_void) -> *mut c_void {
        let probe = argument.cast::<BroadcastProbe>();
        unsafe {
            let mutex = &raw mut (*probe).mutex;
            let condition = &raw mut (*probe).condition;
            assert_eq!(xila_mutex_lock(mutex, 0), Status::Ok);
            assert_eq!(
                xila_condition_wait(condition, mutex, 0, ptr::null()),
                Status::Ok
            );
            (*probe).woken += 1;
            assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
        }
        ptr::null_mut()
    }

    #[test]
    fn probe_destroy_after_broadcast() {
        for reinit in [false, true] {
            let result = run_probe(move || unsafe {
                let process = ptr::dangling::<Process>();
                bind_process(process);
                let mut probe = BroadcastProbe {
                    mutex: core::mem::zeroed(),
                    condition: core::mem::zeroed(),
                    woken: 0,
                };
                let probe = &raw mut probe;
                let mutex = &raw mut (*probe).mutex;
                let condition = &raw mut (*probe).condition;
                let mut ids = [0; 3];
                for id in &mut ids {
                    assert_eq!(
                        xila_thread_spawn(process, 0, Some(probe_waiter), probe.cast(), id),
                        Status::Ok
                    );
                }
                assert_eq!(xila_thread_yield(), Status::Ok);
                assert_eq!(xila_mutex_lock(mutex, 0), Status::Ok);
                assert_eq!(xila_condition_notify(condition, true), Status::Ok);
                assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
                assert_eq!(xila_condition_destroy(condition), Status::Ok);
                assert_eq!((*probe).woken, 0);
                if reinit {
                    assert_eq!(xila_condition_init(condition), Status::Ok);
                }
                for id in ids {
                    assert_eq!(xila_thread_join(id, ptr::null_mut()), Status::Ok);
                }
                (*probe).woken as i32
            });
            assert_eq!(result, 3);
        }
    }

    struct StarvationProbe {
        mutex: Mutex,
        iterations: usize,
    }

    unsafe extern "C" fn hog(argument: *mut c_void) -> *mut c_void {
        let probe = argument.cast::<StarvationProbe>();
        unsafe {
            let mutex = &raw mut (*probe).mutex;
            for _ in 0..50 {
                assert_eq!(xila_mutex_lock(mutex, 0), Status::Ok);
                // Suspend while holding the mutex, as stdio can during a write.
                assert_eq!(xila_thread_yield(), Status::Ok);
                (*probe).iterations += 1;
                assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
            }
        }
        ptr::null_mut()
    }

    #[test]
    fn probe_mutex_starvation() {
        let seen = run_probe(|| unsafe {
            let process = ptr::dangling::<Process>();
            bind_process(process);
            let mut probe = StarvationProbe {
                mutex: core::mem::zeroed(),
                iterations: 0,
            };
            let probe = &raw mut probe;
            let mutex = &raw mut (*probe).mutex;
            let mut id = 0;
            assert_eq!(
                xila_thread_spawn(process, 0, Some(hog), probe.cast(), &mut id),
                Status::Ok
            );
            assert_eq!(xila_thread_yield(), Status::Ok);
            assert_eq!(xila_mutex_lock(mutex, 0), Status::Ok);
            let seen = (*probe).iterations;
            assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
            assert_eq!(xila_thread_join(id, ptr::null_mut()), Status::Ok);
            seen as i32
        });
        assert!(
            (1..=2).contains(&seen),
            "main acquired after {seen} iterations"
        );
    }

    /// Whether the calling worker's native context, and the libc storage it hands out, are
    /// the `Local` its record owns, rather than something on its own coroutine stack.
    fn context_in_record() -> bool {
        let context = stackful::native_context();
        let Ok((group, local)) = current() else {
            return false;
        };
        let mut libc = ptr::null_mut();
        if unsafe { xila_thread_libc(&mut libc) } != Status::Ok {
            return false;
        }
        group
            .with_worker(local.id, |slot| {
                let worker = slot.as_ref().ok_or(Status::NoSuchTask)?;
                let record = ptr::from_ref::<Local<'static>>(&worker.local);
                Ok(ptr::eq(record.cast(), context)
                    && ptr::eq(local, record)
                    && ptr::eq(libc, local.libc.get()))
            })
            .unwrap_or(false)
    }

    unsafe extern "C" fn local_probe(_: *mut c_void) -> *mut c_void {
        let mut libc = ptr::null_mut();
        assert_eq!(unsafe { xila_thread_libc(&mut libc) }, Status::Ok);
        let before = context_in_record();
        // Main fills the worker list while this worker is suspended, which moves the
        // records: the Local, and the libc storage handed out above, must not move.
        assert_eq!(xila_thread_yield(), Status::Ok);
        let mut moved = ptr::null_mut();
        assert_eq!(unsafe { xila_thread_libc(&mut moved) }, Status::Ok);
        let after = context_in_record() && ptr::eq(libc, moved);
        ptr::without_provenance_mut(usize::from(before) | usize::from(after) << 1)
    }

    unsafe extern "C" fn idle(_: *mut c_void) -> *mut c_void {
        ptr::null_mut()
    }

    /// A worker's Local lives in the record its group owns, not on the worker's coroutine
    /// stack, so an overflow there cannot reach what `xila_process_exit` reads through the
    /// native context (#778).
    #[test]
    fn worker_local_stays_off_the_worker_stack() {
        let result = run_probe(|| unsafe {
            let process = ptr::dangling::<Process>();
            bind_process(process);
            let mut probe = 0;
            assert_eq!(
                xila_thread_spawn(process, 0, Some(local_probe), ptr::null_mut(), &mut probe),
                Status::Ok
            );
            // Let the probe run up to its yield.
            assert_eq!(xila_thread_yield(), Status::Ok);
            let mut fillers = [0; 8];
            for id in &mut fillers {
                assert_eq!(
                    xila_thread_spawn(process, 0, Some(idle), ptr::null_mut(), id),
                    Status::Ok
                );
            }
            let mut checks = ptr::null_mut();
            assert_eq!(xila_thread_join(probe, &mut checks), Status::Ok);
            for id in fillers {
                assert_eq!(xila_thread_join(id, ptr::null_mut()), Status::Ok);
            }
            checks.addr() as i32
        });
        assert_eq!(result, 3);
    }

    unsafe extern "C" fn stop_from_worker(argument: *mut c_void) -> *mut c_void {
        assert_eq!(
            unsafe { xila_thread_stop_others(argument.cast()) },
            Status::Ok
        );
        // The sole cleanup thread must resume even if its flush suspends.
        assert_eq!(xila_thread_yield(), Status::Ok);
        crate::exit_coroutine(37);
    }

    unsafe extern "C" fn stopped_worker(_: *mut c_void) -> *mut c_void {
        panic!("stopped worker resumed");
    }

    #[test]
    fn only_shutdown_caller_can_resume() {
        let result = run_probe(|| unsafe {
            let process = ptr::dangling_mut::<Process>();
            bind_process(process);
            let mut id = 0;
            assert_eq!(
                xila_thread_spawn(process, 0, Some(stop_from_worker), process.cast(), &mut id),
                Status::Ok
            );
            assert_eq!(
                xila_thread_spawn(process, 0, Some(stopped_worker), ptr::null_mut(), &mut id),
                Status::Ok
            );
            assert_eq!(xila_thread_yield(), Status::Ok);
            panic!("main resumed after a worker started shutdown");
        });
        assert_eq!(result, 37);
    }
}
