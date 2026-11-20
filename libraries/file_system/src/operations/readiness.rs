//! Non-consuming I/O readiness and notification across multiple handles.
use alloc::vec::Vec;
use core::{cell::RefCell, marker::PhantomPinned, pin::Pin, task::Waker};
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

use crate::{IoErrorKind, Result};

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ReadyEvents: u16 {
        const READ = 1;
        const WRITE = 2;
        const ERROR = 4;
        const HANGUP = 8;
    }
}

type Registration = (usize, Waker);
static WAITERS: Mutex<CriticalSectionRawMutex, RefCell<Vec<Registration>>> =
    Mutex::new(RefCell::new(Vec::new()));

/// A one-shot registration in the I/O readiness hub. Register before probing
/// sources, re-register on each poll, and drop to cancel. Using the pinned
/// address as the key keeps distinct waiters even when they share a task waker.
#[derive(Default)]
pub struct ReadinessWaiter {
    _pinned: PhantomPinned,
    _identity: u8,
}

impl ReadinessWaiter {
    pub fn register(self: Pin<&Self>, waker: &Waker) -> Result<()> {
        let key = core::ptr::from_ref(self.get_ref()).addr();
        let mut replacement = Some(waker.clone());
        let old = WAITERS.lock(|slots| -> Result<Option<Waker>> {
            let mut slots = slots.borrow_mut();
            if let Some((_, current)) = slots.iter_mut().find(|(id, _)| *id == key) {
                return Ok(replacement
                    .take()
                    .map(|waker| core::mem::replace(current, waker)));
            }
            slots.try_reserve(1).map_err(|_| IoErrorKind::OutOfMemory)?;
            if let Some(waker) = replacement.take() {
                slots.push((key, waker));
            }
            Ok(None)
        })?;
        drop(old);
        Ok(())
    }
}

impl Drop for ReadinessWaiter {
    fn drop(&mut self) {
        let key = core::ptr::from_ref(self).addr();
        let (removed, empty) = WAITERS.lock(|slots| {
            let mut slots = slots.borrow_mut();
            let removed = slots
                .iter()
                .position(|(id, _)| *id == key)
                .map(|index| slots.swap_remove(index));
            let empty = slots.is_empty().then(|| core::mem::take(&mut *slots));
            (removed, empty)
        });
        drop((removed, empty));
    }
}

/// Notify after a source changes state. Drivers must release their state lock
/// before calling this. Waiters always recheck their own sources after a wake.
/// Returns the number of registrations woken.
pub fn notify_readiness() -> usize {
    let waiters = WAITERS.lock(|slots| core::mem::take(&mut *slots.borrow_mut()));
    let count = waiters.len();
    for (_, waker) in waiters {
        waker.wake();
    }
    count
}

/// Number of registrations currently held by the hub.
#[cfg(feature = "test_harness")]
#[doc(hidden)]
pub fn readiness_waiter_count() -> usize {
    WAITERS.lock(|slots| slots.borrow().len())
}
