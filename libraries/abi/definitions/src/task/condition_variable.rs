use core::{cell::RefCell, ffi::c_int, ptr::null};
use synchronization::{
    Arc,
    blocking_mutex::{Mutex as RegistryMutex, raw::CriticalSectionRawMutex},
};
use virtual_file_system::Error;

use crate::{XilaTime, XilaTimeClockIdentifier};

use super::{HandleRegistry, RawMutex};

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use core::time::Duration;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::sync::{Condvar as ThreadConditionVariable, Mutex, PoisonError};

pub type XilaConditionVariable = usize;
pub type XilaConditionVariableAttribute = usize;

const REALTIME_ATTRIBUTE: XilaConditionVariableAttribute = 1;
const MONOTONIC_ATTRIBUTE: XilaConditionVariableAttribute = 2;

struct ConditionVariable {
    clock: XilaTimeClockIdentifier,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    monitor: Mutex<()>,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    signaled: ThreadConditionVariable,
}

impl ConditionVariable {
    fn new(clock: XilaTimeClockIdentifier) -> Self {
        Self {
            clock,
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            monitor: Mutex::new(()),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            signaled: ThreadConditionVariable::new(),
        }
    }

    fn wait(&self, mutex: &RawMutex, deadline: Option<XilaTime>) -> Result<(), Error> {
        let deadline = deadline
            .map(|deadline| {
                let manager = crate::try_runtime_context()
                    .ok_or(time::Error::NotInitialized)?
                    .time_manager;
                Ok::<_, time::Error>((deadline, manager))
            })
            .transpose()?;

        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            let monitor = self.monitor.lock().unwrap_or_else(PoisonError::into_inner);
            let saved = mutex
                .release_for_condition_wait()
                .ok_or(Error::PermissionDenied)?;
            let result = (|| {
                let Some((deadline, manager)) = deadline else {
                    drop(
                        self.signaled
                            .wait(monitor)
                            .unwrap_or_else(PoisonError::into_inner),
                    );
                    return Ok(());
                };
                let mut monitor = monitor;
                loop {
                    let now = crate::time::clock_milliseconds(manager, self.clock)?;
                    let remaining = deadline.saturating_sub(now);
                    if remaining == 0 {
                        return Err(Error::RessourceBusy);
                    }
                    // Recheck absolute deadlines periodically so clock adjustments take effect.
                    let (guard, timeout) = self
                        .signaled
                        .wait_timeout(monitor, Duration::from_millis(remaining.min(100)))
                        .unwrap_or_else(PoisonError::into_inner);
                    monitor = guard;
                    if !timeout.timed_out() {
                        return Ok(());
                    }
                }
            })();
            mutex.reacquire_after_condition_wait(saved);
            result
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // An expired deadline needs no suspension; the mutex is still held.
            if let Some((deadline, manager)) = deadline
                && deadline <= crate::time::clock_milliseconds(manager, self.clock)?
            {
                return Err(Error::RessourceBusy);
            }
            let _ = mutex;
            // A synchronous caller cannot suspend the cooperative executor.
            Err(Error::UnsupportedOperation)
        }
    }

    fn notify(&self, broadcast: bool) {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            // A waiter holds this monitor across mutex release and wait registration.
            let _monitor = self.monitor.lock().unwrap_or_else(PoisonError::into_inner);
            if broadcast {
                self.signaled.notify_all();
            } else {
                self.signaled.notify_one();
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let _ = broadcast;
    }
}

static CONDITION_VARIABLES: RegistryMutex<
    CriticalSectionRawMutex,
    RefCell<HandleRegistry<ConditionVariable>>,
> = RegistryMutex::new(RefCell::new(HandleRegistry::new()));

unsafe fn read_handle(condition_variable: *const XilaConditionVariable) -> Result<usize, Error> {
    if condition_variable.is_null() || !condition_variable.is_aligned() {
        return Err(Error::InvalidParameter);
    }
    Ok(unsafe { condition_variable.read() })
}

unsafe fn get_condition_variable(
    condition_variable: *const XilaConditionVariable,
) -> Result<Arc<ConditionVariable>, Error> {
    let handle = unsafe { read_handle(condition_variable)? };
    CONDITION_VARIABLES
        .lock(|variables| variables.borrow().get(handle))
        .ok_or(Error::InvalidIdentifier)
}

fn result_code(result: Result<(), Error>) -> u32 {
    result.err().map_or(0, u32::from)
}

fn attribute_clock(
    attribute: XilaConditionVariableAttribute,
) -> Result<XilaTimeClockIdentifier, Error> {
    match attribute {
        REALTIME_ATTRIBUTE => Ok(XilaTimeClockIdentifier::Realtime),
        MONOTONIC_ATTRIBUTE => Ok(XilaTimeClockIdentifier::Monotonic),
        _ => Err(Error::InvalidParameter),
    }
}

/// Create a condition variable with the default realtime clock.
/// # Safety
/// The output must be aligned and writable and must not contain a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_new(
    condition_variable: *mut XilaConditionVariable,
) -> u32 {
    unsafe { xila_condition_variable_initialize(condition_variable, null()) }
}

/// Initialize a condition variable with the selected clock or the default realtime clock.
/// # Safety
/// The output must be aligned and writable and must not contain a live handle.
/// A nonnull attribute must be aligned, initialized and readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_initialize(
    condition_variable: *mut XilaConditionVariable,
    attribute: *const XilaConditionVariableAttribute,
) -> u32 {
    if condition_variable.is_null() || !condition_variable.is_aligned() || !attribute.is_aligned() {
        return Error::InvalidParameter.into();
    }
    let clock = if attribute.is_null() {
        XilaTimeClockIdentifier::Realtime
    } else {
        match attribute_clock(unsafe { attribute.read() }) {
            Ok(clock) => clock,
            Err(error) => return error.into(),
        }
    };
    let variable = Arc::new(ConditionVariable::new(clock));
    let handle = CONDITION_VARIABLES.lock(|variables| variables.borrow_mut().insert(variable));
    let Some(handle) = handle else {
        return Error::FileSystem(file_system::Error::NoMemory).into();
    };
    unsafe { condition_variable.write(handle) };
    0
}

/// Destroy a condition variable and clear its handle.
/// # Safety
/// The pointer must be aligned, readable and writable. No other calls may be
/// accessing this condition variable or any copies of its handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_remove(
    condition_variable: *mut XilaConditionVariable,
) -> u32 {
    let handle = match unsafe { read_handle(condition_variable) } {
        Ok(handle) => handle,
        Err(error) => return error.into(),
    };
    let removed = CONDITION_VARIABLES.lock(|variables| variables.borrow_mut().remove(handle));
    if removed.is_none() {
        return Error::InvalidIdentifier.into();
    }
    unsafe { condition_variable.write(0) };
    0
}

unsafe fn wait(
    condition_variable: *const XilaConditionVariable,
    mutex: *const RawMutex,
    deadline: Option<XilaTime>,
) -> Result<(), Error> {
    let condition_variable = unsafe { get_condition_variable(condition_variable)? };
    let mutex = unsafe { RawMutex::from_pointer(mutex) }.ok_or(Error::InvalidParameter)?;
    condition_variable.wait(mutex, deadline)
}

/// Release the mutex while waiting, then reacquire it. The caller must check
/// its predicate after waking. The signaling task must be able to run on a
/// separate native thread.
/// # Safety
/// The condition variable and mutex must remain valid throughout the wait.
/// The caller must own the mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_wait(
    condition_variable: *mut XilaConditionVariable,
    mutex: *mut RawMutex,
) -> u32 {
    result_code(unsafe { wait(condition_variable, mutex, None) })
}

/// Wait until signaled or until an absolute deadline in milliseconds on the
/// selected clock. Expiration returns the resource-busy result with the mutex
/// reacquired.
/// # Safety
/// The condition variable and mutex must remain valid throughout the wait.
/// The caller must own the mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_try_wait(
    condition_variable: *mut XilaConditionVariable,
    mutex: *mut RawMutex,
    time: XilaTime,
) -> u32 {
    result_code(unsafe { wait(condition_variable, mutex, Some(time)) })
}

/// Wake one waiter.
/// # Safety
/// The pointer must address a readable handle that remains valid during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_signal(
    condition_variable: *mut XilaConditionVariable,
) -> u32 {
    result_code(
        unsafe { get_condition_variable(condition_variable) }.map(|condition_variable| {
            condition_variable.notify(false);
        }),
    )
}

/// Wake all waiters.
/// # Safety
/// The pointer must address a readable handle that remains valid during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_broadcast(
    condition_variable: *mut XilaConditionVariable,
) -> u32 {
    result_code(
        unsafe { get_condition_variable(condition_variable) }.map(|condition_variable| {
            condition_variable.notify(true);
        }),
    )
}

/// Initialize an attribute with the default realtime clock.
/// # Safety
/// The output pointer must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_initialize(
    attribute: *mut XilaConditionVariableAttribute,
) -> c_int {
    if attribute.is_null() || !attribute.is_aligned() {
        return u32::from(Error::InvalidParameter) as c_int;
    }
    unsafe { attribute.write(REALTIME_ATTRIBUTE) };
    0
}

/// Set the clock used for timed waits.
/// # Safety
/// The attribute must be aligned, readable and writable. The clock identifier
/// must be a valid enumeration value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_set_clock(
    attribute: *mut XilaConditionVariableAttribute,
    clock: XilaTimeClockIdentifier,
) -> c_int {
    if attribute.is_null()
        || !attribute.is_aligned()
        || attribute_clock(unsafe { attribute.read() }).is_err()
    {
        return u32::from(Error::InvalidParameter) as c_int;
    }
    let value = match clock {
        XilaTimeClockIdentifier::Realtime => REALTIME_ATTRIBUTE,
        XilaTimeClockIdentifier::Monotonic => MONOTONIC_ATTRIBUTE,
    };
    unsafe { attribute.write(value) };
    0
}

/// Invalidate an attribute. Existing condition variables retain their clocks.
/// # Safety
/// The attribute must be aligned, readable and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_condition_variable_attribute_destroy(
    attribute: *mut XilaConditionVariableAttribute,
) -> c_int {
    if attribute.is_null()
        || !attribute.is_aligned()
        || attribute_clock(unsafe { attribute.read() }).is_err()
    {
        return u32::from(Error::InvalidParameter) as c_int;
    }
    unsafe { attribute.write(0) };
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{xila_lock_mutex, xila_unlock_mutex};

    #[test]
    fn condition_variable_lifecycle_and_attribute_validation() {
        let mut attribute = 0;
        assert_eq!(
            unsafe { xila_condition_variable_attribute_initialize(&mut attribute) },
            0
        );
        assert_eq!(
            unsafe {
                xila_condition_variable_attribute_set_clock(
                    &mut attribute,
                    XilaTimeClockIdentifier::Monotonic,
                )
            },
            0
        );
        let mut condition_variable = 0;
        assert_eq!(
            unsafe { xila_condition_variable_initialize(&mut condition_variable, &attribute) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_signal(&mut condition_variable) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_broadcast(&mut condition_variable) },
            0
        );
        let mut copied_handle = condition_variable;
        assert_eq!(
            unsafe { xila_condition_variable_remove(&mut condition_variable) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_signal(&mut copied_handle) },
            u32::from(Error::InvalidIdentifier)
        );
        assert_eq!(
            unsafe { xila_condition_variable_attribute_destroy(&mut attribute) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_initialize(&mut condition_variable, &attribute) },
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            unsafe { xila_condition_variable_initialize(&mut condition_variable, null()) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_remove(&mut condition_variable) },
            0
        );
    }

    #[test]
    fn signal_restores_recursive_mutex_ownership() {
        let mut condition_variable = 0;
        assert_eq!(
            unsafe { xila_condition_variable_new(&mut condition_variable) },
            0
        );
        let mutex = RawMutex::new(true);
        let (ready, waiting) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let condition_variable = &condition_variable;
            let mutex = &mutex;
            scope.spawn(move || {
                let mutex = core::ptr::from_ref(mutex).cast_mut();
                let condition_variable = core::ptr::from_ref(condition_variable).cast_mut();
                assert!(unsafe { xila_lock_mutex(mutex, 1) });
                assert!(unsafe { xila_lock_mutex(mutex, 1) });
                ready.send(()).unwrap();
                assert_eq!(
                    unsafe { xila_condition_variable_wait(condition_variable, mutex) },
                    0
                );
                assert!(unsafe { xila_unlock_mutex(mutex, 1) });
                assert!(!unsafe { xila_lock_mutex(mutex, 2) });
                assert!(unsafe { xila_unlock_mutex(mutex, 1) });
                assert!(!unsafe { xila_unlock_mutex(mutex, 1) });
            });

            waiting.recv().unwrap();
            let mutex = core::ptr::from_ref(mutex).cast_mut();
            while !unsafe { xila_lock_mutex(mutex, 2) } {
                std::thread::yield_now();
            }
            let condition_variable = core::ptr::from_ref(condition_variable).cast_mut();
            assert_eq!(
                unsafe { xila_condition_variable_signal(condition_variable) },
                0
            );
            assert!(unsafe { xila_unlock_mutex(mutex, 2) });
        });
        assert_eq!(
            unsafe { xila_condition_variable_remove(&mut condition_variable) },
            0
        );
    }
}
