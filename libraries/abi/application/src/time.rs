use crate::Status;
use synchronization::once_lock::OnceLock;

static TIME: OnceLock<&'static time::Manager<'static>> = OnceLock::new();
/// Install the runtime's clock once. Earlier reads return Unavailable.
pub fn initialize(time_manager: &'static time::Manager<'static>) {
    TIME.get_or_init(|| time_manager);
}

/// Read monotonic nanoseconds from an arbitrary epoch; may return Overflow.
/// # Safety
/// Output must be aligned and writable.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
