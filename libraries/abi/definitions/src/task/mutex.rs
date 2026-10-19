use core::{
    mem::{align_of, size_of},
    ptr::drop_in_place,
};
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use task::TaskIdentifier;

use crate::XilaTaskIdentifier;

#[derive(Debug, Clone, Copy, Default)]
struct MutexState {
    task: Option<TaskIdentifier>,
    lock_count: u32, // For recursive mutexes
}

pub struct RawMutex {
    mutex: Mutex<CriticalSectionRawMutex, MutexState>,
    recursive: bool,
}

impl RawMutex {
    pub fn new(recursive: bool) -> Self {
        Self {
            mutex: Mutex::new(MutexState::default()),
            recursive,
        }
    }

    pub fn is_valid_pointer(pointer: *const RawMutex) -> bool {
        !pointer.is_null() && (pointer as usize).is_multiple_of(align_of::<Self>())
    }

    /// Transforms a pointer to a reference.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences a raw pointer.
    /// The caller must ensure the pointer is valid and points to properly initialized memory.
    pub unsafe fn from_pointer<'a>(pointer: *const RawMutex) -> Option<&'a Self> {
        unsafe {
            if !Self::is_valid_pointer(pointer) {
                return None;
            }
            Some(&*pointer)
        }
    }

    /// Transforms a mutable pointer to a mutable reference.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences a raw pointer.
    /// The caller must ensure the pointer is valid and points to properly initialized memory.
    pub unsafe fn from_mutable_pointer<'a>(pointer: *mut RawMutex) -> Option<&'a mut Self> {
        unsafe {
            if !Self::is_valid_pointer(pointer) {
                return None;
            }
            Some(&mut *pointer)
        }
    }

    pub fn lock(&self, task: TaskIdentifier) -> bool {
        unsafe {
            self.mutex.lock_mut(|state| {
                if let Some(owner) = state.task {
                    if owner == task && self.recursive {
                        // Recursive lock
                        state.lock_count += 1;
                        return true;
                    }
                    // Mutex is already locked by another task
                    return false;
                }

                // Lock is available
                state.task = Some(task);
                state.lock_count = 1;
                true
            })
        }
    }

    pub fn unlock(&self, task: TaskIdentifier) -> bool {
        unsafe {
            self.mutex.lock_mut(|state| {
                // Check if current task owns the mutex
                if let Some(owner) = state.task
                    && owner == task
                {
                    if self.recursive && state.lock_count > 1 {
                        // Decrement lock count for recursive mutex
                        state.lock_count -= 1;
                    } else {
                        // Unlock the mutex
                        state.task = None;
                        state.lock_count = 0;
                    }
                    return true; // Successfully unlocked
                }
                false // Not owned by current task or not locked
            })
        }
    }
}

#[unsafe(no_mangle)]
pub static RAW_MUTEX_SIZE: usize = size_of::<RawMutex>();

/// This function is used to initialize a mutex.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
///
/// # Errors
///
/// This function may return an error if the mutex is not initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_initialize_mutex(mutex: *mut RawMutex) -> bool {
    unsafe {
        if mutex.is_null() {
            return false;
        }

        if !(mutex as usize).is_multiple_of(align_of::<RawMutex>()) {
            return false;
        }

        mutex.write(RawMutex::new(false));

        true
    }
}

/// Initialize a recursive mutex.
///
/// # Safety
///
/// The caller must ensure:
/// - `mutex` points to valid, uninitialized memory
/// - The memory is properly aligned for `Raw_mutex_type`
/// - The memory will remain valid for the lifetime of the mutex
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_initialize_recursive_mutex(mutex: *mut RawMutex) -> bool {
    unsafe {
        if mutex.is_null() {
            return false;
        }

        if !(mutex as usize).is_multiple_of(align_of::<RawMutex>()) {
            return false;
        }

        mutex.write(RawMutex::new(true));

        true
    }
}

/// Lock a mutex (blocking).
///
/// # Safety
///
/// The caller must ensure:
/// - `mutex` points to a valid, initialized `Raw_mutex_type`
/// - The mutex remains valid for the duration of the call
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_lock_mutex(mutex: *mut RawMutex, task: XilaTaskIdentifier) -> bool {
    unsafe {
        let mutex = match RawMutex::from_mutable_pointer(mutex) {
            Some(mutex) => mutex,
            None => return false,
        };

        mutex.lock(task.into())
    }
}

/// Unlock a mutex (blocking).
///
/// # Safety
///
/// The caller must ensure:
/// - `mutex` points to a valid, initialized `Raw_mutex_type`
/// - The mutex remains valid for the duration of the call
/// - The current task owns the mutex
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_unlock_mutex(mutex: *mut RawMutex, task: XilaTaskIdentifier) -> bool {
    unsafe {
        let mutex = match RawMutex::from_mutable_pointer(mutex) {
            Some(mutex) => mutex,
            None => return false,
        };

        mutex.unlock(task.into())
    }
}

/// Destroy a mutex.
///
/// # Safety
///
/// The caller must ensure:
/// - `mutex` points to a valid, initialized `Raw_mutex_type` allocated with Box
/// - The mutex is not currently locked
/// - No other threads are waiting on the mutex
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_destroy_mutex(mutex: *mut RawMutex) -> bool {
    unsafe {
        let mutex = match RawMutex::from_mutable_pointer(mutex) {
            Some(mutex) => mutex,
            None => return false,
        };

        // Drop the mutex, which will release any resources it holds
        drop_in_place(mutex);

        true // Mutex is dropped here
    }
}
