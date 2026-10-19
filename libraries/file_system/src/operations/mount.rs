use core::ops::{Deref, DerefMut};

use alloc::boxed::Box;
use synchronization::{
    blocking_mutex::raw::RawMutex,
    mutex::{Mutex, MutexGuard},
};

use crate::{Error, Result};

pub trait MountOperations {
    fn mount(&self) -> Result<()> {
        Ok(())
    }

    fn unmount(&self) -> Result<()> {
        Err(Error::UnsupportedOperation)
    }
}
pub struct MountWrapper<T>(Option<Box<T>>);

impl<T> Default for MountWrapper<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> MountWrapper<T> {
    pub const fn new() -> Self {
        Self(None)
    }

    pub fn mount(&mut self, inner: T) -> Result<()> {
        if self.is_mounted() {
            return Err(Error::AlreadyMounted);
        }
        self.0 = Some(Box::new(inner));

        Ok(())
    }

    pub fn unmount(&mut self) -> Result<()> {
        if !self.is_mounted() {
            return Err(Error::NotMounted);
        }

        self.0 = None;
        Ok(())
    }

    pub const fn is_mounted(&self) -> bool {
        self.0.is_some()
    }

    pub fn try_get_mutable(&mut self) -> Result<&mut T> {
        match &mut self.0 {
            Some(inner) => Ok(inner.as_mut()),
            None => Err(Error::NotMounted),
        }
    }

    pub fn try_get(&self) -> Result<&T> {
        match &self.0 {
            Some(inner) => Ok(inner.as_ref()),
            None => Err(Error::NotMounted),
        }
    }
}

pub struct MutexMountWrapperGuard<'a, M: RawMutex, T> {
    _guard: MutexGuard<'a, M, MountWrapper<T>>,
    inner: &'a mut T,
}

impl<M: RawMutex, T> Deref for MutexMountWrapperGuard<'_, M, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<M: RawMutex, T> DerefMut for MutexMountWrapperGuard<'_, M, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

pub struct MutexMountWrapper<M: RawMutex, T>(Mutex<M, MountWrapper<T>>);

impl<M: RawMutex, T> Default for MutexMountWrapper<M, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: RawMutex, T> MutexMountWrapper<M, T> {
    pub const fn new() -> Self {
        Self(Mutex::new(MountWrapper::new()))
    }

    pub fn new_mounted(inner: T) -> Self {
        Self(Mutex::new({
            let mut wrapper = MountWrapper::new();
            wrapper.mount(inner).unwrap();
            wrapper
        }))
    }

    pub fn mount(&self, inner: T) -> Result<()> {
        let mut guard = self.0.try_lock().map_err(|_| Error::RessourceBusy)?;
        guard.mount(inner)
    }

    pub fn unmount(&self) -> Result<()> {
        let mut guard = self.0.try_lock().map_err(|_| Error::RessourceBusy)?;
        guard.unmount()
    }

    pub fn is_mounted(&self) -> Result<bool> {
        let guard = self.0.try_lock().map_err(|_| Error::RessourceBusy)?;
        Ok(guard.is_mounted())
    }

    pub fn try_get(&self) -> Result<MutexMountWrapperGuard<'_, M, T>> {
        let mut guard = self.0.try_lock().map_err(|_| Error::RessourceBusy)?;

        // Verify mount before creating the guard
        if !guard.is_mounted() {
            return Err(Error::NotMounted);
        }

        let inner_ptr = guard.try_get_mutable()? as *mut T;

        Ok(MutexMountWrapperGuard {
            _guard: guard,
            inner: unsafe { &mut *inner_ptr },
        })
    }
}
