use core::{
    mem::{align_of, size_of},
    ptr::drop_in_place,
};
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use task::TaskIdentifier;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::sync::{Condvar as ConditionVariable, Mutex as ThreadMutex, PoisonError};

use crate::XilaTaskIdentifier;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct MutexState {
    task: Option<TaskIdentifier>,
    lock_count: u32, // For recursive mutexes
}

pub struct RawMutex {
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    mutex: Mutex<CriticalSectionRawMutex, MutexState>,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    mutex: ThreadMutex<MutexState>,
    recursive: bool,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    released: ConditionVariable,
}

impl RawMutex {
    pub fn new(recursive: bool) -> Self {
        Self {
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            mutex: Mutex::new(MutexState::default()),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            mutex: ThreadMutex::new(MutexState::default()),
            recursive,
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            released: ConditionVariable::new(),
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

    fn with_state<T>(&self, operation: impl FnOnce(&mut MutexState) -> T) -> T {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        return operation(&mut self.mutex.lock().unwrap_or_else(PoisonError::into_inner));

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        // State operations never reenter this mutex.
        unsafe {
            self.mutex.lock_mut(operation)
        }
    }

    pub fn lock(&self, task: TaskIdentifier) -> bool {
        self.with_state(|state| {
            if let Some(owner) = state.task {
                if owner != task || !self.recursive {
                    return false;
                }
                let Some(count) = state.lock_count.checked_add(1) else {
                    return false;
                };
                state.lock_count = count;
            } else {
                state.task = Some(task);
                state.lock_count = 1;
            }
            true
        })
    }

    pub fn unlock(&self, task: TaskIdentifier) -> bool {
        self.with_state(|state| {
            if state.task != Some(task) {
                return false;
            }
            state.lock_count -= 1;
            if state.lock_count == 0 {
                state.task = None;
                #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
                self.released.notify_one();
            }
            true
        })
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    pub(super) fn release_for_condition_wait(&self) -> Option<MutexState> {
        self.with_state(|state| {
            state.task?;
            let saved = core::mem::take(state);
            self.released.notify_one();
            Some(saved)
        })
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    pub(super) fn reacquire_after_condition_wait(&self, saved: MutexState) {
        let state = self.mutex.lock().unwrap_or_else(PoisonError::into_inner);
        let mut state = self
            .released
            .wait_while(state, |state| state.task.is_some())
            .unwrap_or_else(PoisonError::into_inner);
        *state = saved;
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
        let mutex = match RawMutex::from_pointer(mutex) {
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
        let mutex = match RawMutex::from_pointer(mutex) {
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
