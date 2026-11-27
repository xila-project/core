//! The task being polled on the current host thread or processor core.

use core::sync::atomic::Ordering;
use portable_atomic::{AtomicU8, AtomicU32};

use crate::TaskIdentifier;

const NAME_CAPACITY: usize = 32;

#[derive(Clone, Copy)]
pub struct ActiveTask {
    pub identifier: u32,
    name: [u8; NAME_CAPACITY],
    length: u8,
}

impl ActiveTask {
    pub(crate) fn new(identifier: TaskIdentifier, name: &str) -> Self {
        let length = name.floor_char_boundary(NAME_CAPACITY);
        let mut bytes = [0; NAME_CAPACITY];
        bytes[..length].copy_from_slice(&name.as_bytes()[..length]);
        Self {
            identifier: usize::from(identifier) as u32,
            name: bytes,
            length: length as u8,
        }
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..usize::from(self.length)]).unwrap_or_default()
    }

    pub(crate) fn enter(self) -> Restore {
        Restore(
            with_slot(|slot| {
                let previous = slot.snapshot();
                slot.replace(Some(self));
                previous
            })
            .flatten(),
        )
    }
}

pub(crate) struct Restore(Option<ActiveTask>);

impl Drop for Restore {
    fn drop(&mut self) {
        with_slot(|slot| slot.replace(self.0));
    }
}

/// Read the active task without acquiring a lock. Returns `None` outside a
/// managed task poll or when capture interrupts a slot update.
pub fn snapshot() -> Option<ActiveTask> {
    with_slot(Slot::snapshot).flatten()
}

struct Slot {
    length: AtomicU8,
    identifier: AtomicU32,
    name: [AtomicU32; NAME_CAPACITY / 4],
}

impl Slot {
    const EMPTY: u8 = u8::MAX;

    const fn new() -> Self {
        Self {
            length: AtomicU8::new(Self::EMPTY),
            identifier: AtomicU32::new(0),
            name: [const { AtomicU32::new(0) }; NAME_CAPACITY / 4],
        }
    }

    fn snapshot(&self) -> Option<ActiveTask> {
        let length = self.length.load(Ordering::SeqCst);
        if length == Self::EMPTY {
            return None;
        }
        let mut name = [0; NAME_CAPACITY];
        for (destination, source) in name.chunks_exact_mut(4).zip(&self.name) {
            destination.copy_from_slice(&source.load(Ordering::Relaxed).to_ne_bytes());
        }
        Some(ActiveTask {
            identifier: self.identifier.load(Ordering::Relaxed),
            name,
            length,
        })
    }

    fn replace(&self, task: Option<ActiveTask>) {
        // The writer owns this slot. A panic or signal can interrupt the
        // writer on the same core or thread and sees no active task until the
        // data is complete.
        self.length.store(Self::EMPTY, Ordering::SeqCst);
        if let Some(task) = task {
            self.identifier.store(task.identifier, Ordering::Relaxed);
            for (destination, source) in self.name.iter().zip(task.name.chunks_exact(4)) {
                destination.store(
                    u32::from_ne_bytes(source.try_into().unwrap()),
                    Ordering::Relaxed,
                );
            }
            self.length.store(task.length, Ordering::SeqCst);
        }
    }
}

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
std::thread_local! {
    static CURRENT: Slot = const { Slot::new() };
}

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
fn with_slot<T>(function: impl FnOnce(&Slot) -> T) -> Option<T> {
    CURRENT.try_with(function).ok()
}

/// Register a lock-free core identifier on bare metal before running tasks.
/// Each core must return its own index in the range `0..2`. The callback
/// must be safe to use from panic and interrupt contexts.
#[cfg(not(any(test, target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn set_core_identifier_provider(provider: fn() -> usize) -> bool {
    CORE_IDENTIFIER
        .compare_exchange(
            core::ptr::null_mut(),
            provider as *mut (),
            Ordering::Release,
            Ordering::Relaxed,
        )
        .is_ok()
}

#[cfg(not(any(test, target_os = "linux", target_os = "macos", target_os = "windows")))]
static CORE_IDENTIFIER: portable_atomic::AtomicPtr<()> =
    portable_atomic::AtomicPtr::new(core::ptr::null_mut());

#[cfg(not(any(test, target_os = "linux", target_os = "macos", target_os = "windows")))]
fn with_slot<T>(function: impl FnOnce(&Slot) -> T) -> Option<T> {
    static CURRENT: [Slot; 2] = [const { Slot::new() }; 2];
    let provider = CORE_IDENTIFIER.load(Ordering::Acquire);
    let index = if provider.is_null() {
        0
    } else {
        // SAFETY: Registration stores a function pointer with this signature.
        let provider: fn() -> usize = unsafe { core::mem::transmute(provider) };
        provider()
    };
    CURRENT.get(index).map(function)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_nested_tasks_and_truncates_names_at_character_boundaries() {
        let outer = ActiveTask::new(TaskIdentifier::new(1), "outer");
        let inner = ActiveTask::new(TaskIdentifier::new(2), "1234567890123456789012345678901é");
        assert!(snapshot().is_none());
        {
            let _outer = outer.enter();
            {
                let _inner = inner.enter();
                let active = snapshot().unwrap();
                assert_eq!(active.identifier, 2);
                assert_eq!(active.name(), "1234567890123456789012345678901");
            }
            assert_eq!(snapshot().unwrap().name(), "outer");
        }
        assert!(snapshot().is_none());
    }
}
