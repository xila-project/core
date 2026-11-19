use crate::{Status, is_in_coroutine, suspend_on};
use core::time::Duration;
use synchronization::once_lock::OnceLock;

static TIME: OnceLock<&'static time::Manager<'static>> = OnceLock::new();
/// Install the runtime's clock once. Earlier reads return Unavailable.
pub fn initialize(time_manager: &'static time::Manager<'static>) {
    TIME.get_or_init(|| time_manager);
}

/// Read monotonic nanoseconds from an arbitrary epoch; may return Overflow.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_time_monotonic(nanoseconds: *mut u64) -> Status {
    if nanoseconds.is_null() {
        return Status::InvalidArgument;
    }
    let Some(time) = TIME.try_get() else {
        return Status::Unavailable;
    };
    match u64::try_from(time.monotonic_now().as_nanos()) {
        Ok(value) => {
            unsafe { nanoseconds.write(value) };
            Status::Ok
        }
        Err(_) => Status::Overflow,
    }
}
/// Read signed Unix UTC nanoseconds; unsynchronized clocks return Unavailable.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_time_realtime(nanoseconds: *mut i64) -> Status {
    if nanoseconds.is_null() {
        return Status::InvalidArgument;
    }
    let Some(time) = TIME.try_get() else {
        return Status::Unavailable;
    };
    match time.realtime_now() {
        Ok(value) => match i64::try_from(value.as_nanos()) {
            Ok(value) => {
                unsafe { nanoseconds.write(value) };
                Status::Ok
            }
            Err(_) => Status::Overflow,
        },
        Err(time::Error::Overflow) => Status::Overflow,
        Err(_) => Status::Unavailable,
    }
}

/// Monotonic clock identifier for `xila_time_resolution`.
pub const XILA_CLOCK_MONOTONIC: u32 = 0;
/// Realtime (Unix UTC) clock identifier for `xila_time_resolution`.
pub const XILA_CLOCK_REALTIME: u32 = 1;

/// Read a clock's nominal quantization in nanoseconds. Actual precision can be
/// coarser. Realtime resolution stays readable while UTC is unsynchronized.
/// An unknown clock identifier returns InvalidArgument; an uninstalled clock
/// returns Unavailable. Outputs change only on success.
/// # Safety
/// Output must be aligned and writable.
pub unsafe extern "C" fn xila_time_resolution(clock: u32, nanoseconds: *mut u64) -> Status {
    if nanoseconds.is_null() || clock > XILA_CLOCK_REALTIME {
        return Status::InvalidArgument;
    }
    let Some(time) = TIME.try_get() else {
        return Status::Unavailable;
    };
    let resolution = if clock == XILA_CLOCK_MONOTONIC {
        time.monotonic_resolution()
    } else {
        time.realtime_resolution()
    };
    match u64::try_from(resolution.as_nanos()) {
        Ok(value) => {
            unsafe { nanoseconds.write(value) };
            Status::Ok
        }
        Err(_) => Status::Overflow,
    }
}

/// Suspend the caller for at least the given nanoseconds on the monotonic clock.
/// Inside a coroutine this suspends it on the executor's timer, so other tasks
/// run meanwhile; zero yields once. Outside a coroutine there is no executor
/// waker for the timer, so the call polls the clock and zero returns at once.
/// UTC corrections do not affect the wait.
pub extern "C" fn xila_time_sleep(nanoseconds: u64) -> Status {
    let duration = Duration::from_nanos(nanoseconds);
    if is_in_coroutine() {
        // An Embassy timer always yields once before it can complete.
        suspend_on(task::sleep(duration));
    } else {
        let start = time::MonotonicClock::now(&time::EmbassyClock);
        while time::MonotonicClock::now(&time::EmbassyClock).saturating_sub(start) < duration {
            core::hint::spin_loop();
        }
    }
    Status::Ok
}
