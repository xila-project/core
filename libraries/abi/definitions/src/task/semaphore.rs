use core::ffi::{c_char, c_int};

use crate::{XilaFileSystemAccess, XilaFileSystemOpen};

pub struct XilaSemaphore;

/// This function is used to create a semaphore.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_open(
    _name: *const c_char,
    _open: XilaFileSystemOpen,
    _mode: XilaFileSystemAccess,
    _value: isize,
) -> *mut XilaSemaphore {
    todo!()
}

/// This function is used to close a semaphore.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_close(_semaphore: *mut XilaSemaphore) -> u32 {
    let _ = _semaphore;
    todo!()
}

/// This function is used to wait a semaphore.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_wait(_semaphore: *mut XilaSemaphore) -> u32 {
    todo!()
}

/// This function is used to try wait a semaphore.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_try_wait(_semaphore: *mut XilaSemaphore) -> u32 {
    todo!()
}

/// This function is used to post a semaphore.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_post(_semaphore: *mut XilaSemaphore) -> u32 {
    todo!()
}

/// This function is used to get the value of a semaphore.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_get_value(
    _semaphore: *mut XilaSemaphore,
    _value: *mut c_int,
) -> u32 {
    todo!()
}

/// This function is used to get the value of a semaphore.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_remove(_name: *const c_char) -> u32 {
    todo!()
}
