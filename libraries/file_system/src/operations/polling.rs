//! Pollable I/O operation identities and synchronous boundary adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Read,
    Write,
    Flush,
    Control,
}

#[cfg(test)]
mod tests {
    use crate::{BaseOperations, Context, DirectBaseOperations, Error, Operation, Result, Size};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context as TaskContext, Poll, Waker},
    };
    struct Device(AtomicUsize);
    impl DirectBaseOperations for Device {
        fn read(&self, _: &mut TaskContext<'_>, _: &mut [u8], _: Size) -> Poll<Result<usize>> {
            Poll::Pending
        }
        fn write(&self, _: &mut TaskContext<'_>, _: &[u8], _: Size) -> Poll<Result<usize>> {
            Poll::Ready(Err(Error::InputOutput))
        }
        fn cancel_operation(&self, _: Operation) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn base_read_itself_is_pollable_through_dynamic_dispatch() {
        let device = Device(AtomicUsize::new(0));
        let base: &dyn BaseOperations = &device;
        let mut context = Context::new_empty();
        let mut cx = TaskContext::from_waker(Waker::noop());
        assert!(base.read(&mut context, &mut cx, &mut [0], 0).is_pending());
        assert_eq!(
            base.write(&mut context, &mut cx, b"x", 0),
            Poll::Ready(Err(Error::InputOutput))
        );
        base.cancel_operation(&mut context, Operation::Read);
        assert_eq!(device.0.load(Ordering::Relaxed), 1);
    }
}

use core::{cell::RefCell, task::Waker};
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// One opening's reservation in a WaitQueue. Never share or clone reservations
/// between openings; always cancel before discarding an opening.
#[derive(Default)]
pub struct Waiter {
    slot: Option<usize>,
}
impl Waiter {
    pub const fn new() -> Self {
        Self { slot: None }
    }
}
struct Slot {
    reserved: bool,
    waker: Option<Waker>,
}

/// Bounded, allocation-free registrations. Notifications preserve reservations
/// until cancellation, preventing a woken opening from clearing a reused slot.
/// Callers must check readiness, register, then recheck before returning Pending.
pub struct WaitQueue<const N: usize> {
    slots: Mutex<CriticalSectionRawMutex, RefCell<[Slot; N]>>,
}
impl<const N: usize> Default for WaitQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> WaitQueue<N> {
    pub const fn new() -> Self {
        Self {
            slots: Mutex::new(RefCell::new(
                [const {
                    Slot {
                        reserved: false,
                        waker: None,
                    }
                }; N],
            )),
        }
    }
    pub fn register(&self, waiter: &mut Waiter, waker: &Waker) -> crate::Result<()> {
        let mut replacement = Some(waker.clone());
        let result = self.slots.lock(|slots| {
            let mut slots = slots.borrow_mut();
            let index = match waiter.slot {
                Some(index) => {
                    if index >= N || !slots[index].reserved {
                        return Err(crate::Error::InvalidContext);
                    }
                    index
                }
                None => {
                    let index = slots
                        .iter()
                        .position(|s| !s.reserved)
                        .ok_or(crate::Error::TooManyOpenFiles)?;
                    slots[index].reserved = true;
                    waiter.slot = Some(index);
                    index
                }
            };
            Ok(slots[index].waker.replace(replacement.take().unwrap()))
        });
        drop(replacement);
        let retired = result?;
        drop(retired);
        Ok(())
    }
    pub fn cancel(&self, waiter: &mut Waiter) {
        let Some(index) = waiter.slot.take() else {
            return;
        };
        let retired = self.slots.lock(|slots| {
            let mut slots = slots.borrow_mut();
            let slot = slots.get_mut(index)?;
            slot.reserved = false;
            slot.waker.take()
        });
        drop(retired);
    }
    pub fn wake_all(&self) {
        let mut wake: [Option<Waker>; N] = core::array::from_fn(|_| None);
        self.slots.lock(|slots| {
            for (slot, out) in slots.borrow_mut().iter_mut().zip(&mut wake) {
                *out = slot.waker.take();
            }
        });
        for waker in wake.into_iter().flatten() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod wait_tests {
    extern crate std;
    use crate::{Error, WaitQueue, Waiter};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Waker,
    };
    use std::{sync::Arc, task::Wake};
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn independent_waiters_are_bounded_and_cancelled_slots_can_be_reused() {
        let queue = WaitQueue::<2>::new();
        let mut first = Waiter::new();
        let mut second = Waiter::new();
        let mut third = Waiter::new();
        let count = Arc::new(Count::default());
        let waker = Waker::from(count.clone());
        queue.register(&mut first, &waker).unwrap();
        queue.register(&mut second, &waker).unwrap();
        assert_eq!(
            queue.register(&mut third, &waker),
            Err(Error::TooManyOpenFiles)
        );
        queue.wake_all();
        assert_eq!(count.0.load(Ordering::Relaxed), 2);
        queue.cancel(&mut first);
        queue.register(&mut third, &waker).unwrap();
        queue.cancel(&mut first);
        queue.wake_all();
        assert_eq!(count.0.load(Ordering::Relaxed), 3);
    }
}
