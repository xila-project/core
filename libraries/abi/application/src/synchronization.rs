use crate::Status;
#[cfg(target_os = "linux")]
use alloc::boxed::Box;
#[cfg(target_os = "linux")]
extern crate std;

pub const XILA_MUTEX_RECURSIVE: u32 = 1;

/// Opaque native mutex. Ownership is per OS thread, not per Xila task.
pub struct Mutex {
    #[cfg(target_os = "linux")]
    state: std::sync::Mutex<State>,
    #[cfg(target_os = "linux")]
    available: std::sync::Condvar,
    #[cfg(target_os = "linux")]
    recursive: bool,
}
#[cfg(target_os = "linux")]
struct State {
    owner: Option<std::thread::ThreadId>,
    depth: u32,
}
/// Opaque condition variable. Always wait in a predicate loop.
pub struct ConditionVariable {
    #[cfg(target_os = "linux")]
    inner: std::sync::Condvar,
}
#[cfg(target_os = "linux")]
impl Mutex {
    fn acquire(&self, blocking: bool) -> Status {
        let id = std::thread::current().id();
        let mut state = self.state.lock().unwrap();
        if state.owner == Some(id) && self.recursive {
            let Some(depth) = state.depth.checked_add(1) else {
                return Status::Overflow;
            };
            state.depth = depth;
            return Status::Ok;
        }
        if state.owner == Some(id) && blocking {
            return Status::Deadlock;
        }
        while state.owner.is_some() {
            if !blocking {
                return Status::WouldBlock;
            }
            state = self.available.wait(state).unwrap();
        }
        state.owner = Some(id);
        state.depth = 1;
        Status::Ok
    }
}
#[cfg(target_os = "linux")]
fn wait(condition: &ConditionVariable, mutex: &Mutex, timeout: Option<u64>) -> Status {
    let id = std::thread::current().id();
    let mut state = mutex.state.lock().unwrap();
    if state.owner != Some(id) || state.depth != 1 {
        return Status::InvalidArgument;
    }
    let duration = timeout.map(core::time::Duration::from_nanos);
    if let Some(duration) = duration
        && std::time::Instant::now().checked_add(duration).is_none()
    {
        return Status::Overflow;
    }
    state.owner = None;
    state.depth = 0;
    mutex.available.notify_one();
    let expired;
    if let Some(duration) = duration {
        let (new_state, result) = condition.inner.wait_timeout(state, duration).unwrap();
        state = new_state;
        expired = result.timed_out();
    } else {
        state = condition.inner.wait(state).unwrap();
        expired = false;
    }
    while state.owner.is_some() {
        state = mutex.available.wait(state).unwrap();
    }
    state.owner = Some(id);
    state.depth = 1;
    if expired {
        Status::TimedOut
    } else {
        Status::Ok
    }
}

/// Allocate a mutex once; unknown flags are invalid.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_mutex_create(flags: u32, mutex: *mut *mut Mutex) -> Status {
    if mutex.is_null() || flags & !XILA_MUTEX_RECURSIVE != 0 {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            mutex.write(Box::into_raw(Box::new(Mutex {
                state: std::sync::Mutex::new(State {
                    owner: None,
                    depth: 0,
                }),
                available: std::sync::Condvar::new(),
                recursive: flags != 0,
            })))
        };
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = flags;
        Status::Unsupported
    }
}

/// Lock, sleeping while contended. Nonrecursive self-lock returns Deadlock.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_mutex_lock(mutex: *mut Mutex) -> Status {
    if mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*mutex };
        value.acquire(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mutex;
        Status::Unsupported
    }
}

/// Probe once; contention returns WouldBlock.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_mutex_try_lock(mutex: *mut Mutex) -> Status {
    if mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*mutex };
        value.acquire(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mutex;
        Status::Unsupported
    }
}

/// Unlock; only the owning native thread may unlock.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_mutex_unlock(mutex: *mut Mutex) -> Status {
    if mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*mutex };
        let mut state = value.state.lock().unwrap();
        if state.owner != Some(std::thread::current().id()) {
            return Status::PermissionDenied;
        }
        state.depth -= 1;
        if state.depth == 0 {
            state.owner = None;
            value.available.notify_one();
        }
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mutex;
        Status::Unsupported
    }
}

/// Free an unlocked mutex. Busy leaves ownership with the caller.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_mutex_destroy(mutex: *mut Mutex) -> Status {
    if mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*mutex };
        if value.state.lock().unwrap().owner.is_some() {
            return Status::Busy;
        }
        drop(unsafe { Box::from_raw(mutex) });
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mutex;
        Status::Unsupported
    }
}

/// Allocate a condition variable.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_create(condition: *mut *mut ConditionVariable) -> Status {
    if condition.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            condition.write(Box::into_raw(Box::new(ConditionVariable {
                inner: std::sync::Condvar::new(),
            })))
        };
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = condition;
        Status::Unsupported
    }
}

/// Atomically release and reacquire mutex (held exactly once). Timeout also reacquires it.
/// All waits on this condition variable must use the same mutex.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_wait(
    condition: *mut ConditionVariable,
    mutex: *mut Mutex,
) -> Status {
    if condition.is_null() || mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let condition = unsafe { &*condition };
        let mutex = unsafe { &*mutex };
        wait(condition, mutex, None)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (condition, mutex);
        Status::Unsupported
    }
}

/// Atomically release and reacquire mutex (held exactly once). Timeout also reacquires it.
/// All waits on this condition variable must use the same mutex.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_wait_timeout(
    condition: *mut ConditionVariable,
    mutex: *mut Mutex,
    nanoseconds: u64,
) -> Status {
    if condition.is_null() || mutex.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let condition = unsafe { &*condition };
        let mutex = unsafe { &*mutex };
        wait(condition, mutex, Some(nanoseconds))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (condition, mutex, nanoseconds);
        Status::Unsupported
    }
}

/// Wake one waiter; notifications are not saved.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_signal(condition: *mut ConditionVariable) -> Status {
    if condition.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*condition };
        value.inner.notify_one();
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = condition;
        Status::Unsupported
    }
}

/// Wake all current waiters.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_broadcast(condition: *mut ConditionVariable) -> Status {
    if condition.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*condition };
        value.inner.notify_all();
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = condition;
        Status::Unsupported
    }
}

/// Free a condition variable with no waiters.
/// # Safety
/// Objects must be live, aligned and remain live throughout the call. Outputs
/// must be writable and disjoint from inputs. Destroy requires exclusive ownership,
/// no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condvar_destroy(condition: *mut ConditionVariable) -> Status {
    if condition.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*condition };
        let _ = value;
        drop(unsafe { Box::from_raw(condition) });
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = condition;
        Status::Unsupported
    }
}
