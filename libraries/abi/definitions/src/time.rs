use core::ffi::{c_int, c_void};
use virtual_file_system::Error;

pub type XilaTime = u64;

#[repr(C)]
pub struct XilaTimePrecision {
    seconds: XilaTime,
    nanoseconds: u32,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum XilaTimeClockIdentifier {
    Monotonic,
    Realtime,
}

#[repr(u8)]
pub enum XilaTimerFlags {
    Absolute,
}

/// Retrieve the current time since the system startup in microseconds.
///
/// # Returns
///
/// The current time since the system startup in microseconds.
#[unsafe(no_mangle)]
pub extern "C" fn xila_time_get_time_since_startup_microseconds() -> u64 {
    // Saturate rather than silently wrapping at the ABI's u64 boundary.
    u64::try_from(crate::runtime_context().time_manager.uptime().as_micros()).unwrap_or(u64::MAX)
}

/// Return the current thread's processor time in nanoseconds, or zero when
/// accounting is unavailable. This interface has no separate status result.
#[unsafe(no_mangle)]
pub extern "C" fn xila_time_get_cpu() -> u64 {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut value = core::mem::MaybeUninit::uninit();
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, value.as_mut_ptr()) } == 0 {
            let value = unsafe { value.assume_init() };
            let nanoseconds = i128::from(value.tv_sec) * 1_000_000_000 + i128::from(value.tv_nsec);
            return u64::try_from(nanoseconds).unwrap_or_default();
        }
    }
    // Other targets have no thread processor clock in the runtime.
    0
}

fn write_clock_value(
    output: *mut XilaTime,
    read: impl FnOnce(&time::Manager<'static>) -> time::Result<XilaTime>,
) -> u32 {
    if output.is_null() || !output.is_aligned() {
        return Error::InvalidParameter.into();
    }
    let Some(runtime) = crate::try_runtime_context() else {
        return Error::Time(time::Error::NotInitialized).into();
    };
    match read(runtime.time_manager) {
        Ok(value) => {
            unsafe { output.write(value) };
            0
        }
        Err(error) => Error::Time(error).into(),
    }
}

pub(crate) fn clock_milliseconds(
    manager: &time::Manager<'_>,
    clock: XilaTimeClockIdentifier,
) -> time::Result<XilaTime> {
    match clock {
        XilaTimeClockIdentifier::Monotonic => u64::try_from(manager.monotonic_now().as_millis()),
        XilaTimeClockIdentifier::Realtime => {
            let duration = manager.realtime_now()?.try_into_duration()?;
            u64::try_from(duration.as_millis())
        }
    }
    .map_err(|_| time::Error::Overflow)
}

/// Return the selected clock's nominal resolution, rounded up to milliseconds.
/// # Safety
/// The output must be aligned and writable and the clock identifier must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_time_get_resolution(
    clock_identifier: XilaTimeClockIdentifier,
    resolution: *mut XilaTime,
) -> u32 {
    write_clock_value(resolution, |manager| {
        let duration = match clock_identifier {
            XilaTimeClockIdentifier::Monotonic => manager.monotonic_resolution(),
            XilaTimeClockIdentifier::Realtime => manager.realtime_resolution(),
        };
        u64::try_from(duration.as_nanos().div_ceil(1_000_000)).map_err(|_| time::Error::Overflow)
    })
}

/// Return monotonic time from the clock's epoch or realtime from the Unix epoch,
/// in whole milliseconds. The precision argument is a hint and does not change
/// the time unit.
/// # Safety
/// The output must be aligned and writable and the clock identifier must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_time_get_time(
    clock_identifier: XilaTimeClockIdentifier,
    _precision: u64,
    time: *mut XilaTime,
) -> u32 {
    write_clock_value(time, |manager| {
        clock_milliseconds(manager, clock_identifier)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn xila_time_nano_sleep(
    _clock_identifier: XilaTimeClockIdentifier,
    _flags: XilaTimerFlags,
    _time: *const c_void,
    _remaining: *mut XilaTime,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn xila_time_dummy(_: XilaTimePrecision) -> XilaTime {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null_mut;
    use virtual_file_system::Error;

    #[test]
    fn time_reads_reject_null_outputs() {
        assert_eq!(
            unsafe { xila_time_get_resolution(XilaTimeClockIdentifier::Monotonic, null_mut()) },
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            unsafe { xila_time_get_time(XilaTimeClockIdentifier::Realtime, 0, null_mut()) },
            u32::from(Error::InvalidParameter)
        );
    }

    #[task::test]
    async fn clock_values_and_expired_condition_deadlines_use_milliseconds() {
        use crate::{
            RawMutex, RuntimeContext, xila_condition_variable_attribute_initialize,
            xila_condition_variable_attribute_set_clock, xila_condition_variable_initialize,
            xila_condition_variable_remove, xila_condition_variable_try_wait,
        };
        use alloc::boxed::Box;
        use core::time::Duration;

        struct FixedClock;
        impl time::MonotonicClock for FixedClock {
            fn now(&self) -> Duration {
                Duration::from_micros(12_345)
            }

            fn resolution(&self) -> Duration {
                Duration::from_micros(250)
            }
        }

        let manager = Box::leak(Box::new(time::Manager::with_clock(&FixedClock)));
        let tasks = Box::leak(Box::new(task::Manager::new()));
        let users = Box::leak(Box::new(users::Manager::new()));
        let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let file_system =
            little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
        let virtual_file_system = Box::leak(Box::new(
            virtual_file_system::initialize(tasks, users, manager, file_system).unwrap(),
        ));
        crate::initialize(RuntimeContext {
            virtual_file_system,
            time_manager: manager,
        });

        let mut value = 999;
        assert_eq!(
            unsafe { xila_time_get_time(XilaTimeClockIdentifier::Realtime, 0, &mut value) },
            u32::from(Error::Time(time::Error::Unsynchronized))
        );
        assert_eq!(value, 999);
        assert_eq!(
            unsafe { xila_time_get_time(XilaTimeClockIdentifier::Monotonic, 100, &mut value) },
            0
        );
        assert_eq!(value, 12);
        assert_eq!(
            unsafe { xila_time_get_resolution(XilaTimeClockIdentifier::Monotonic, &mut value) },
            0
        );
        assert_eq!(value, 1);

        manager
            .set_realtime(time::RealtimeSample {
                timestamp: time::Timestamp::from_nanos(1_234_567_890),
                source: time::ClockSource::Manual,
                resolution: Duration::from_micros(1_500),
                accuracy: None,
            })
            .unwrap();
        assert_eq!(
            unsafe { xila_time_get_time(XilaTimeClockIdentifier::Realtime, 0, &mut value) },
            0
        );
        assert_eq!(value, 1_234);
        assert_eq!(
            unsafe { xila_time_get_resolution(XilaTimeClockIdentifier::Realtime, &mut value) },
            0
        );
        assert_eq!(value, 2);

        let mut attribute = 0;
        let mut condition_variable = 0;
        let mut mutex = RawMutex::new(false);
        assert!(mutex.lock(1usize.into()));
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
        assert_eq!(
            unsafe { xila_condition_variable_initialize(&mut condition_variable, &attribute) },
            0
        );
        assert_eq!(
            unsafe { xila_condition_variable_try_wait(&mut condition_variable, &mut mutex, 12) },
            u32::from(Error::RessourceBusy)
        );
        assert!(mutex.unlock(1usize.into()));
        assert_eq!(
            unsafe { xila_condition_variable_remove(&mut condition_variable) },
            0
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn thread_processor_time_is_available() {
        assert!(xila_time_get_cpu() > 0);
    }
}
