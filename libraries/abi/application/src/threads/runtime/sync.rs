use super::{Group, current, status};
use crate::{
    Condition, Mutex, Status, XILA_CLOCK_MONOTONIC, XILA_CLOCK_REALTIME, XILA_MUTEX_RECURSIVE,
    XILA_MUTEX_TRY, suspend_dynamic, xila_time_monotonic, xila_time_realtime,
};
use core::{
    ffi::c_void,
    future::{Future, poll_fn},
    pin::{Pin, pin},
    ptr,
    task::Poll,
};
use embassy_time::Timer;

/// Intrusive FIFO wait queue. Nodes live in the suspended calls, so a wait
/// allocates nothing. Only the process driver ever polls these coroutines. All
/// of them stop on cancellation; no queue or waker in another task can refer
/// to these nodes.
struct Waiter {
    next: *mut Waiter,
    owner: usize,
    notified: bool,
}

/// Register before suspending (and, for condvars, before unlocking). Each poll
/// checks notification before the deadline. Neither holds a Rust reference
/// into the caller's synchronization object across the suspension.
unsafe fn wait(
    queue: *mut *mut c_void,
    owner: usize,
    mut timer: Option<Timer>,
    release: impl FnOnce(),
) -> Result<(), Status> {
    let mut waiter = Waiter {
        next: ptr::null_mut(),
        owner,
        notified: false,
    };
    let node = ptr::from_mut(&mut waiter);
    let mut tail = queue.cast::<*mut Waiter>();
    unsafe {
        while !(*tail).is_null() {
            tail = &raw mut (**tail).next;
        }
        tail.write(node);
    }
    release();

    let mut result = Ok(());
    suspend_dynamic(pin!(poll_fn(|cx| {
        if unsafe { (*node).notified } {
            Poll::Ready(())
        } else if timer
            .as_mut()
            .is_some_and(|timer| Pin::new(timer).poll(cx).is_ready())
        {
            result = Err(Status::TimedOut);
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })));

    // Notification removes the node from the queue. Only a timeout still
    // owns a queued node and may access the condition variable.
    if result.is_err() {
        let mut link = queue.cast::<*mut Waiter>();
        unsafe {
            while *link != node {
                link = &raw mut (**link).next;
            }
            link.write((*node).next);
        }
    }
    result
}

fn pop_waiter(queue: &mut *mut c_void) -> Option<*mut Waiter> {
    let waiter = queue.cast::<Waiter>();
    if waiter.is_null() {
        return None;
    }
    // Queued nodes belong to suspended calls in the current process.
    *queue = unsafe { (*waiter).next.cast() };
    Some(waiter)
}

/// Initialize an uninitialized or destroyed mutex.
/// # Safety
/// Mutex must be aligned, writable and unused.
pub unsafe extern "C" fn xila_mutex_init(mutex: *mut Mutex, flags: u32) -> Status {
    if mutex.is_null() || flags & !XILA_MUTEX_RECURSIVE != 0 {
        return Status::InvalidArgument;
    }
    unsafe {
        mutex.write(Mutex {
            owner: 0,
            depth: 0,
            recursive: flags == XILA_MUTEX_RECURSIVE,
            waiters: ptr::null_mut(),
        })
    };
    Status::Ok
}

/// Destroy a mutex, returning ResourceBusy while owned or in use by a waiter.
/// # Safety
/// Mutex must be live and cannot be used again until reinitialized.
pub unsafe extern "C" fn xila_mutex_destroy(mutex: *mut Mutex) -> Status {
    match unsafe { mutex.as_ref() } {
        Some(mutex) if mutex.owner == 0 && mutex.waiters.is_null() => Status::Ok,
        Some(_) => Status::ResourceBusy,
        None => Status::InvalidArgument,
    }
}

unsafe fn lock(mutex: *mut Mutex, owner: usize, try_only: bool) -> Result<(), Status> {
    let state = unsafe { mutex.as_mut() }.ok_or(Status::InvalidArgument)?;
    match state.owner {
        0 => {
            state.owner = owner;
            state.depth = 1;
            Ok(())
        }
        id if id == owner && state.recursive => {
            state.depth = state.depth.checked_add(1).ok_or(Status::Overflow)?;
            Ok(())
        }
        _ if try_only => Err(Status::WouldBlock),
        id if id == owner => Err(Status::Deadlock),
        // Unlock gives this waiter ownership before waking it.
        _ => unsafe { wait(&raw mut (*mutex).waiters, owner, None, || {}) },
    }
}

fn unlock(group: &Group, mutex: &mut Mutex) {
    mutex.depth -= 1;
    if mutex.depth == 0 {
        if let Some(waiter) = pop_waiter(&mut mutex.waiters) {
            unsafe {
                mutex.owner = (*waiter).owner;
                mutex.depth = 1;
                (*waiter).notified = true;
            }
            group.notify();
        } else {
            mutex.owner = 0;
        }
    }
}

/// Acquire a mutex, suspending on contention unless XILA_MUTEX_TRY is set.
/// # Safety
/// Mutex must remain live at the same address throughout the call.
pub unsafe extern "C" fn xila_mutex_lock(mutex: *mut Mutex, flags: u32) -> Status {
    status((|| {
        let (_, local) = current()?;
        if flags & !XILA_MUTEX_TRY != 0 {
            return Err(Status::InvalidArgument);
        }
        unsafe { lock(mutex, local.id, flags == XILA_MUTEX_TRY) }
    })())
}

/// Release one level of the caller's ownership.
/// # Safety
/// Mutex must be live.
pub unsafe extern "C" fn xila_mutex_unlock(mutex: *mut Mutex) -> Status {
    status((|| {
        let (group, local) = current()?;
        let mutex = unsafe { mutex.as_mut() }.ok_or(Status::InvalidArgument)?;
        if mutex.owner != local.id {
            return Err(Status::OperationNotPermitted);
        }
        unlock(group, mutex);
        Ok(())
    })())
}

/// Initialize an uninitialized or destroyed condition variable.
/// # Safety
/// Condition must be aligned, writable and unused.
pub unsafe extern "C" fn xila_condition_init(condition: *mut Condition) -> Status {
    if condition.is_null() {
        return Status::InvalidArgument;
    }
    unsafe {
        condition.write(Condition {
            waiters: ptr::null_mut(),
        })
    };
    Status::Ok
}

/// Destroy a condition variable, returning ResourceBusy if there are waiters.
/// # Safety
/// Condition must be live and cannot be used again until reinitialized.
pub unsafe extern "C" fn xila_condition_destroy(condition: *mut Condition) -> Status {
    match unsafe { condition.as_ref() } {
        Some(condition) if condition.waiters.is_null() => Status::Ok,
        Some(_) => Status::ResourceBusy,
        None => Status::InvalidArgument,
    }
}

fn deadline_timer(clock: u32, deadline: i64) -> Result<Timer, Status> {
    let now = match clock {
        XILA_CLOCK_MONOTONIC => {
            let mut now = 0;
            let status = unsafe { xila_time_monotonic(&mut now) };
            if status != Status::Ok {
                return Err(status);
            }
            i128::from(now)
        }
        XILA_CLOCK_REALTIME => {
            let mut now = 0;
            let status = unsafe { xila_time_realtime(&mut now) };
            if status != Status::Ok {
                return Err(status);
            }
            i128::from(now)
        }
        _ => return Err(Status::InvalidArgument),
    };
    let duration = (i128::from(deadline) - now).max(0) as u64;
    Ok(Timer::at(task::deadline_after_nanoseconds(duration)))
}

/// Wait for notification or an absolute clock deadline, reacquiring the mutex
/// before returning. The caller must own the mutex with depth exactly one.
/// # Safety
/// The mutex must remain live at a fixed address until the call returns.
/// The condition must remain live at a fixed address until this waiter is
/// notified or times out.
/// A non-null deadline must be aligned and readable.
pub unsafe extern "C" fn xila_condition_wait(
    condition: *mut Condition,
    mutex: *mut Mutex,
    clock: u32,
    deadline: *const i64,
) -> Status {
    status((|| {
        let (group, local) = current()?;
        if condition.is_null() || mutex.is_null() {
            return Err(Status::InvalidArgument);
        }
        let state = unsafe { &*mutex };
        if state.owner != local.id {
            return Err(Status::OperationNotPermitted);
        }
        if state.depth != 1 {
            return Err(Status::InvalidArgument);
        }
        let timer = unsafe { deadline.as_ref() }
            .map(|deadline| deadline_timer(clock, *deadline))
            .transpose()?;
        // No suspension point between registration and unlock.
        let waited = unsafe {
            wait(&raw mut (*condition).waiters, local.id, timer, || {
                unlock(group, &mut *mutex)
            })
        };
        unsafe { lock(mutex, local.id, false)? };
        waited
    })())
}

/// Notify one waiter, or all waiters if all is true.
/// # Safety
/// Condition must be live.
pub unsafe extern "C" fn xila_condition_notify(condition: *mut Condition, all: bool) -> Status {
    status((|| {
        let (group, _) = current()?;
        let condition = unsafe { condition.as_mut() }.ok_or(Status::InvalidArgument)?;
        while let Some(waiter) = pop_waiter(&mut condition.waiters) {
            unsafe { (*waiter).notified = true };
            group.notify();
            if !all {
                break;
            }
        }
        Ok(())
    })())
}
