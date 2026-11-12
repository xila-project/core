use crate::Status;
use core::ffi::{c_char, c_void};
#[cfg(target_os = "linux")]
extern crate std;

/// Extensible thread configuration. Zero stack size selects the platform default.
#[repr(C)]
pub struct ThreadOptions {
    pub size: u32,
    pub stack_size: usize,
    pub name: *const c_char,
    pub name_length: usize,
}

/// Owned native thread join token.
pub struct Thread {
    #[cfg(target_os = "linux")]
    handle: std::thread::JoinHandle<i32>,
}

/// Start a native OS thread. Unsupported on targets without a stackful backend.
/// # Safety
/// Options must be readable and aligned, including its declared size. Name must
/// be readable for its length. Output must be writable and disjoint from inputs.
/// Entry and argument must remain valid until entry returns, including after detach.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_thread_spawn(
    entry: Option<extern "C" fn(*mut c_void) -> i32>,
    argument: *mut c_void,
    options: *const ThreadOptions,
    thread: *mut *mut Thread,
) -> Status {
    let Some(entry) = entry else {
        return Status::InvalidArgument;
    };
    if thread.is_null() {
        return Status::InvalidArgument;
    }
    let mut stack_size = 0;
    let mut name = None;
    if !options.is_null() {
        // Read only the leading field until its size has been validated.
        if unsafe { options.cast::<u32>().read() } < core::mem::size_of::<ThreadOptions>() as u32 {
            return Status::InvalidArgument;
        }
        let options = unsafe { &*options };
        stack_size = options.stack_size;
        if stack_size > isize::MAX as usize {
            return Status::InvalidArgument;
        }
        if options.name_length != 0 {
            let value = match unsafe { crate::path_input(options.name, options.name_length) } {
                Ok(value) if !value.contains('\0') => value,
                _ => return Status::InvalidArgument,
            };
            name = Some(value);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut builder = std::thread::Builder::new();
        if stack_size != 0 {
            builder = builder.stack_size(stack_size);
        }
        if let Some(name) = name {
            builder = builder.name(alloc::string::String::from(name));
        }
        // The trusted caller guarantees the argument is transferable and live.
        let argument = argument as usize;
        match builder.spawn(move || entry(argument as *mut c_void)) {
            Ok(handle) => {
                unsafe {
                    thread.write(alloc::boxed::Box::into_raw(alloc::boxed::Box::new(
                        Thread { handle },
                    )))
                };
                Status::Ok
            }
            Err(_) => Status::Unavailable,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (entry, argument, stack_size, name);
        Status::Unsupported
    }
}

/// Join and consume a thread. Validation and self-join errors do not consume it.
/// # Safety
/// Thread must be uniquely owned and live. Result must be aligned, writable and
/// disjoint from thread. No concurrent join/detach is permitted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_thread_join(thread: *mut Thread, result: *mut i32) -> Status {
    if thread.is_null() || result.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        let value = unsafe { &*thread };
        if value.handle.thread().id() == std::thread::current().id() {
            return Status::Deadlock;
        }
        let value = unsafe { alloc::boxed::Box::from_raw(thread) };
        match value.handle.join() {
            Ok(value) => {
                unsafe { result.write(value) };
                Status::Ok
            }
            Err(_) => Status::Unavailable,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Status::Unsupported
    }
}

/// Consume a join token; the thread continues and releases its resources on exit.
/// # Safety
/// Thread must be uniquely owned and live, with no concurrent join/detach.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_thread_detach(thread: *mut Thread) -> Status {
    if thread.is_null() {
        return Status::InvalidArgument;
    }
    #[cfg(target_os = "linux")]
    {
        drop(unsafe { alloc::boxed::Box::from_raw(thread) });
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        Status::Unsupported
    }
}

/// Sleep for at least the supplied duration using the OS scheduler.
/// # Safety
/// Call on a dedicated native thread, not inside a cooperative executor poll.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_thread_sleep(nanoseconds: u64) -> Status {
    #[cfg(target_os = "linux")]
    {
        std::thread::sleep(core::time::Duration::from_nanos(nanoseconds));
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = nanoseconds;
        Status::Unsupported
    }
}

/// Yield the calling OS thread to the scheduler.
/// # Safety
/// No preconditions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_thread_yield() -> Status {
    #[cfg(target_os = "linux")]
    {
        std::thread::yield_now();
        Status::Ok
    }
    #[cfg(not(target_os = "linux"))]
    {
        Status::Unsupported
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::*;
    use core::ptr;

    extern "C" fn entry(_: *mut c_void) -> i32 {
        -37
    }

    #[test]
    fn options_ownership_and_validation() {
        unsafe {
            let mut thread = ptr::null_mut();
            let mut options = ThreadOptions {
                size: 0,
                stack_size: 65536,
                name: c"native".as_ptr(),
                name_length: 6,
            };
            assert_eq!(
                xila_thread_spawn(Some(entry), ptr::null_mut(), &options, &mut thread),
                Status::InvalidArgument
            );
            assert!(thread.is_null());
            options.size = core::mem::size_of::<ThreadOptions>() as u32;
            options.name_length = 7;
            assert_eq!(
                xila_thread_spawn(Some(entry), ptr::null_mut(), &options, &mut thread),
                Status::InvalidArgument
            );
            options.name_length = 6;
            assert_eq!(
                xila_thread_spawn(None, ptr::null_mut(), &options, &mut thread),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_thread_spawn(Some(entry), ptr::null_mut(), &options, &mut thread),
                Status::Ok
            );
            assert_eq!(
                xila_thread_join(thread, ptr::null_mut()),
                Status::InvalidArgument
            );
            let mut result = 0;
            assert_eq!(xila_thread_join(thread, &mut result), Status::Ok);
            assert_eq!(result, -37);
            let mut mutex = ptr::null_mut();
            assert_eq!(xila_mutex_create(2, &mut mutex), Status::InvalidArgument);
            assert!(mutex.is_null());
            assert_eq!(
                xila_mutex_create(XILA_MUTEX_RECURSIVE, &mut mutex),
                Status::Ok
            );
            assert_eq!(xila_mutex_unlock(mutex), Status::PermissionDenied);
            assert_eq!(xila_mutex_lock(mutex), Status::Ok);
            assert_eq!(xila_mutex_try_lock(mutex), Status::Ok);
            let mut condition = ptr::null_mut();
            assert_eq!(xila_condvar_create(&mut condition), Status::Ok);
            assert_eq!(
                xila_condvar_wait_timeout(condition, mutex, 0),
                Status::InvalidArgument
            );
            assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
            assert_eq!(xila_mutex_unlock(mutex), Status::Ok);
            assert_eq!(xila_mutex_destroy(mutex), Status::Ok);
            assert_eq!(xila_condvar_destroy(condition), Status::Ok);
            assert_eq!(xila_mutex_lock(ptr::null_mut()), Status::InvalidArgument);
            assert_eq!(xila_thread_detach(ptr::null_mut()), Status::InvalidArgument);
        }
        std::println!(
            "Thread={} Mutex={} ConditionVariable={} ThreadOptions={}",
            core::mem::size_of::<Thread>(),
            core::mem::size_of::<Mutex>(),
            core::mem::size_of::<ConditionVariable>(),
            core::mem::size_of::<ThreadOptions>()
        );
    }
}
