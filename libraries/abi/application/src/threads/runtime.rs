use super::*;
use crate::{Process, Status, suspend_dynamic};
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell, UnsafeCell},
    future::poll_fn,
    pin::pin,
    ptr,
    task::{Poll, Waker},
};
use stackful::{Coroutine, Stack};

mod sync;
pub use sync::*;

const MAIN_THREAD: usize = 1;

struct Local {
    group: *const Group,
    id: usize,
    libc: UnsafeCell<ThreadLibc>,
    values: [Cell<(usize, *mut c_void)>; XILA_THREAD_KEYS_MAX],
}

impl Local {
    const fn new(group: &Group, id: usize) -> Self {
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
}

struct Worker {
    id: usize,
    creator: usize,
    coroutine: Option<Coroutine>,
    result: Option<*mut c_void>,
    detached: bool,
    joining: bool,
}

struct Group {
    process: Cell<*const Process>,
    workers: RefCell<Vec<Option<Worker>>>,
    next_thread: Cell<usize>,
    keys: [Cell<usize>; XILA_THREAD_KEYS_MAX],
    next_key: Cell<usize>,
    repoll: Cell<bool>,
    stopping: Cell<bool>,
    exit: Cell<Option<i32>>,
}

impl Group {
    const fn new() -> Self {
        Self {
            process: Cell::new(ptr::null()),
            workers: RefCell::new(Vec::new()),
            next_thread: Cell::new(MAIN_THREAD + 1),
            keys: [const { Cell::new(0) }; XILA_THREAD_KEYS_MAX],
            next_key: Cell::new(XILA_THREAD_KEYS_MAX),
            repoll: Cell::new(false),
            stopping: Cell::new(false),
            exit: Cell::new(None),
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
    fn poll_workers(&self, waker: &Waker) {
        let count = self.workers.borrow().len();
        for index in 0..count {
            // Release the borrow before resuming arbitrary C (which can spawn).
            let Some(mut coroutine) = self.workers.borrow_mut()[index]
                .as_mut()
                .and_then(|worker| worker.coroutine.take())
            else {
                continue;
            };
            let finished = coroutine
                .resume(&mut core::task::Context::from_waker(waker))
                .is_ready();
            if self.stopping.get() {
                return;
            }
            let mut workers = self.workers.borrow_mut();
            let slot = &mut workers[index];
            if let Some(worker) = slot {
                if !finished {
                    worker.coroutine = Some(coroutine);
                    continue;
                }
                let id = worker.id;
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
    }

    fn key(&self, key: usize) -> Result<&Cell<usize>, Status> {
        let slot = &self.keys[key % XILA_THREAD_KEYS_MAX];
        if key == 0 || slot.get() != key {
            return Err(Status::InvalidArgument);
        }
        Ok(slot)
    }
}

fn current() -> Result<(&'static Group, &'static Local), Status> {
    // Only drive_process and its worker entry wrapper install this context.
    // Both keep their Local and its group live for the entire C execution.
    let local = unsafe { stackful::native_context().cast::<Local>().as_ref() }
        .ok_or(Status::Unsupported)?;
    Ok((unsafe { &*local.group }, local))
}

pub(crate) fn bind_process(process: *const Process) {
    if let Ok((group, _)) = current() {
        group.process.set(process);
    }
}

pub(crate) fn stop_workers() {
    if let Ok((group, _)) = current() {
        group.stopping.set(true);
    }
}

pub(crate) fn exit_process(process: *const Process, status: i32) -> Result<(), Status> {
    if let Ok((group, local)) = current() {
        if group.process.get() != process {
            return Err(Status::InvalidArgument);
        }
        group.stopping.set(true);
        if local.id != MAIN_THREAD {
            group.exit.set(Some(status));
            // This worker has no host-owned values live below its C entry.
            crate::exit_coroutine(status);
        }
    }
    Ok(())
}

/// Drive the process entry and all its workers on the same executor task.
/// The group and its workers are dropped before this future returns. A
/// suspended coroutine follows stackful's cancellation policy (#731).
pub async fn drive_process(mut main: Coroutine) -> i32 {
    let group = Group::new();
    let local = Local::new(&group, MAIN_THREAD);
    poll_fn(|cx| {
        // Both locals are borrowed by this pinned future and stay at fixed
        // addresses until every coroutine has permanently stopped.
        unsafe { main.set_native_context(ptr::from_ref(&local).cast()) };
        group.repoll.set(false);

        if let Poll::Ready(status) = main.resume(cx) {
            return Poll::Ready(status);
        }
        if !group.stopping.get() {
            group.poll_workers(cx.waker());
        }
        if group.stopping.get() {
            group.workers.borrow_mut().clear();
        }
        if let Some(status) = group.exit.get() {
            return Poll::Ready(status);
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
        if group.stopping.get() {
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
        let mut workers = group.workers.borrow_mut();
        let index = if let Some(index) = workers.iter().position(Option::is_none) {
            index
        } else {
            workers.try_reserve(1).map_err(|_| Status::OutOfMemory)?;
            workers.push(None);
            workers.len() - 1
        };
        let coroutine = unsafe {
            Coroutine::new(stack, move || {
                let local = Local::new(group, id);
                stackful::set_native_context(ptr::from_ref(&local).cast());
                let result = entry(argument);
                if let Some(worker) = &mut group.workers.borrow_mut()[index] {
                    worker.result = Some(result);
                }
                0
            })
        };
        workers[index] = Some(Worker {
            id,
            creator: local.id,
            coroutine: Some(coroutine),
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
    let mut yielded = false;
    suspend_dynamic(pin!(poll_fn(|cx| {
        if core::mem::replace(&mut yielded, true) {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })));
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
}
