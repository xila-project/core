use alloc::{collections::BTreeMap, vec::Vec};
use core::{
    ffi::{CStr, c_char, c_int},
    ptr::{null_mut, without_provenance_mut},
};
use file_system::{AccessFlags, CreateFlags};
use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
};
use virtual_file_system::Error;

use crate::{XilaFileSystemAccess, XilaFileSystemOpen};

use super::HandleRegistry;

pub struct XilaSemaphore;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::sync::{Condvar as ConditionVariable, Mutex as ThreadMutex, PoisonError};

struct Semaphore {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    permits: ThreadMutex<c_int>,
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    available: ConditionVariable,
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    permits: Mutex<CriticalSectionRawMutex, core::cell::Cell<c_int>>,
}

impl Semaphore {
    fn new(permits: c_int) -> Self {
        Self {
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            permits: ThreadMutex::new(permits),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            available: ConditionVariable::new(),
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            permits: Mutex::new(core::cell::Cell::new(permits)),
        }
    }

    fn with_permits<T>(&self, operation: impl FnOnce(&mut c_int) -> T) -> T {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        return operation(&mut self.permits.lock().unwrap_or_else(PoisonError::into_inner));

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        self.permits.lock(|permits| {
            let mut count = permits.get();
            let result = operation(&mut count);
            permits.set(count);
            result
        })
    }

    fn wait(&self) -> u32 {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            let permits = self.permits.lock().unwrap_or_else(PoisonError::into_inner);
            let mut permits = self
                .available
                .wait_while(permits, |permits| *permits == 0)
                .unwrap_or_else(PoisonError::into_inner);
            *permits -= 1;
            0
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // A synchronous caller cannot suspend the cooperative executor.
            self.try_wait()
        }
    }

    fn try_wait(&self) -> u32 {
        self.with_permits(|permits| {
            if *permits == 0 {
                return Error::RessourceBusy.into();
            }
            *permits -= 1;
            0
        })
    }

    fn post(&self) -> u32 {
        self.with_permits(|permits| {
            let Some(incremented) = permits.checked_add(1) else {
                return Error::InvalidParameter.into();
            };
            *permits = incremented;
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            self.available.notify_one();
            0
        })
    }
}

struct SemaphoreRegistry {
    names: BTreeMap<Vec<u8>, Arc<Semaphore>>,
    handles: HandleRegistry<Semaphore>,
}

static SEMAPHORES: Mutex<CriticalSectionRawMutex, core::cell::RefCell<SemaphoreRegistry>> =
    Mutex::new(core::cell::RefCell::new(SemaphoreRegistry {
        names: BTreeMap::new(),
        handles: HandleRegistry::new(),
    }));

fn with_semaphore(semaphore: *mut XilaSemaphore, operation: impl FnOnce(&Semaphore) -> u32) -> u32 {
    let semaphore = SEMAPHORES.lock(|registry| registry.borrow().handles.get(semaphore.addr()));
    match semaphore {
        Some(semaphore) => operation(&semaphore),
        None => Error::InvalidIdentifier.into(),
    }
}

/// Open a named semaphore, returning null on failure.
/// Names are local to the runtime. There is no per-user permission model for them.
/// # Safety
/// The name must be a readable, null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_open(
    name: *const c_char,
    open: XilaFileSystemOpen,
    mode: XilaFileSystemAccess,
    value: isize,
) -> *mut XilaSemaphore {
    if name.is_null() || AccessFlags::from_bits(mode).is_none() {
        return null_mut();
    }
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    let Some(flags) = CreateFlags::from_bits(open) else {
        return null_mut();
    };
    if name.is_empty() || !CreateFlags::CREATE_EXCLUSIVE.contains(flags) {
        return null_mut();
    }

    SEMAPHORES.lock(|registry| {
        let mut registry = registry.borrow_mut();
        let (semaphore, created) = match registry.names.get(name) {
            Some(_) if flags.contains(CreateFlags::CREATE_EXCLUSIVE) => return null_mut(),
            Some(semaphore) => (semaphore.clone(), false),
            None if flags.contains(CreateFlags::Create) => {
                let Ok(permits) = c_int::try_from(value) else {
                    return null_mut();
                };
                if permits < 0 {
                    return null_mut();
                }
                (Arc::new(Semaphore::new(permits)), true)
            }
            None => return null_mut(),
        };

        let Some(handle) = registry.handles.insert(semaphore.clone()) else {
            return null_mut();
        };
        if created {
            registry.names.insert(name.to_vec(), semaphore);
        }
        // The handle is an identifier. It is never dereferenced or reused.
        without_provenance_mut(handle)
    })
}

/// Close one handle. The name remains available until it is removed.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_close(semaphore: *mut XilaSemaphore) -> u32 {
    let removed =
        SEMAPHORES.lock(|registry| registry.borrow_mut().handles.remove(semaphore.addr()));
    if removed.is_some() {
        0
    } else {
        Error::InvalidIdentifier.into()
    }
}

/// Take one permit. On hosted targets, the posting caller must run on a
/// different thread if this call blocks. On cooperative targets, an empty
/// semaphore returns the resource-busy result.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_wait(semaphore: *mut XilaSemaphore) -> u32 {
    with_semaphore(semaphore, Semaphore::wait)
}

/// Take one permit if immediately available.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_try_wait(semaphore: *mut XilaSemaphore) -> u32 {
    with_semaphore(semaphore, Semaphore::try_wait)
}

/// Add one permit, up to the maximum value representable by the output integer.
#[unsafe(no_mangle)]
pub extern "C" fn xila_semaphore_post(semaphore: *mut XilaSemaphore) -> u32 {
    with_semaphore(semaphore, Semaphore::post)
}

/// Read the number of currently available permits.
/// # Safety
/// The output must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_get_value(
    semaphore: *mut XilaSemaphore,
    value: *mut c_int,
) -> u32 {
    if value.is_null() || !value.is_aligned() {
        return Error::InvalidParameter.into();
    }
    with_semaphore(semaphore, |semaphore| {
        let permits = semaphore.with_permits(|permits| *permits);
        unsafe { value.write(permits) };
        0
    })
}

/// Remove a name without invalidating open handles.
/// # Safety
/// The name must be a readable, null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_semaphore_remove(name: *const c_char) -> u32 {
    if name.is_null() {
        return Error::InvalidParameter.into();
    }
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    if name.is_empty() {
        return Error::InvalidParameter.into();
    }

    let removed = SEMAPHORES.lock(|registry| registry.borrow_mut().names.remove(name));
    if removed.is_some() {
        0
    } else {
        Error::FileSystem(file_system::Error::NotFound).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null_mut;
    use file_system::{AccessFlags, CreateFlags};

    #[test]
    fn named_semaphore_handles_share_permits_until_removed() {
        let name = c"/shared-permits".as_ptr();
        let access = AccessFlags::READ_WRITE.bits();
        let create = CreateFlags::CREATE_EXCLUSIVE.bits();
        let first = unsafe { xila_semaphore_open(name, create, access, 1) };
        assert_ne!(first, null_mut());
        assert!(unsafe { xila_semaphore_open(name, create, access, 5) }.is_null());
        let second = unsafe { xila_semaphore_open(name, 0, access, -1) };
        assert_ne!(second, null_mut());

        assert_eq!(xila_semaphore_wait(first), 0);
        assert_eq!(
            xila_semaphore_try_wait(second),
            u32::from(Error::RessourceBusy)
        );
        assert_eq!(xila_semaphore_post(second), 0);
        let mut value = -1;
        assert_eq!(unsafe { xila_semaphore_get_value(first, &mut value) }, 0);
        assert_eq!(value, 1);

        assert_eq!(unsafe { xila_semaphore_remove(name) }, 0);
        assert!(unsafe { xila_semaphore_open(name, 0, access, 0) }.is_null());
        let replacement = unsafe { xila_semaphore_open(name, create, access, 0) };
        assert_ne!(replacement, null_mut());
        assert_eq!(xila_semaphore_try_wait(first), 0);
        assert_eq!(
            xila_semaphore_try_wait(replacement),
            u32::from(Error::RessourceBusy)
        );

        assert_eq!(xila_semaphore_close(first), 0);
        assert_eq!(
            xila_semaphore_close(first),
            u32::from(Error::InvalidIdentifier)
        );
        assert_eq!(xila_semaphore_close(second), 0);
        assert_eq!(xila_semaphore_close(replacement), 0);
        assert_eq!(unsafe { xila_semaphore_remove(name) }, 0);
    }

    #[test]
    fn semaphore_post_wakes_a_waiting_thread() {
        let name = c"/blocking-permits".as_ptr();
        let access = AccessFlags::READ_WRITE.bits();
        let semaphore =
            unsafe { xila_semaphore_open(name, CreateFlags::CREATE_EXCLUSIVE.bits(), access, 0) };
        assert!(!semaphore.is_null());
        let (ready, waiting) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let waiter = scope.spawn(move || {
                let semaphore =
                    unsafe { xila_semaphore_open(c"/blocking-permits".as_ptr(), 0, access, 0) };
                assert!(!semaphore.is_null());
                ready.send(()).unwrap();
                assert_eq!(xila_semaphore_wait(semaphore), 0);
                assert_eq!(xila_semaphore_close(semaphore), 0);
            });
            waiting.recv().unwrap();
            assert_eq!(xila_semaphore_post(semaphore), 0);
            waiter.join().unwrap();
        });
        assert_eq!(
            xila_semaphore_try_wait(semaphore),
            u32::from(Error::RessourceBusy)
        );
        assert_eq!(xila_semaphore_close(semaphore), 0);
        assert_eq!(unsafe { xila_semaphore_remove(name) }, 0);
    }
}
