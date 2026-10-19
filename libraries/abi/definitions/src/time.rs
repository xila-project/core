use core::ffi::{c_int, c_void};

pub type XilaTime = u64;

#[repr(C)]
pub struct XilaTimePrecision {
    seconds: XilaTime,
    nanoseconds: u32,
}

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
    crate::runtime_context()
        .time_manager
        .get_current_time_since_startup()
        .unwrap_or_default()
        .as_micros() as u64
}

#[unsafe(no_mangle)]
pub extern "C" fn xila_time_get_cpu() -> u64 {
    todo!()
}

/// Retrieve the current time since the system startup in milliseconds.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_time_get_resolution(
    _clock_identifier: XilaTimeClockIdentifier,
    _resolution: *mut XilaTime,
) -> u32 {
    todo!()
}

/// Retrieve the current time since the system startup in milliseconds.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub extern "C" fn xila_time_get_time(
    _clock_identifier: XilaTimeClockIdentifier,
    _precision: u64,
    _time: *mut XilaTime,
) -> u32 {
    todo!()
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
