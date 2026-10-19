//! Internal operation identities for cancellation; unrelated to descriptor poll/select.

/// One pending operation per mutably borrowed opening context. Duplicates have
/// independent contexts and registrations. `Pending` is not a busy error: it
/// requires a race-free wakeup protocol owned by the device.
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

    struct Busy;
    impl DirectBaseOperations for Busy {
        fn read(&self, _: &mut [u8], _: Size) -> Result<usize> {
            Err(Error::RessourceBusy)
        }
        fn write(&self, _: &[u8], _: Size) -> Result<usize> {
            Err(Error::InputOutput)
        }
    }

    #[test]
    fn defaults_preserve_busy_and_errors() {
        let device: &dyn BaseOperations = &Busy;
        let mut context = Context::new_empty();
        let mut cx = TaskContext::from_waker(Waker::noop());
        assert!(!device.supports_polling());
        assert_eq!(
            device.poll_read(&mut context, &mut cx, &mut [0], 0),
            Poll::Ready(Err(Error::RessourceBusy))
        );
        assert_eq!(
            device.poll_write(&mut context, &mut cx, b"x", 0),
            Poll::Ready(Err(Error::InputOutput))
        );
    }

    struct EventDevice(AtomicUsize);
    impl DirectBaseOperations for EventDevice {
        fn read(&self, _: &mut [u8], _: Size) -> Result<usize> {
            Err(Error::RessourceBusy)
        }
        fn write(&self, _: &[u8], _: Size) -> Result<usize> {
            Err(Error::RessourceBusy)
        }
        fn supports_polling(&self) -> bool {
            true
        }
        fn poll_read(
            &self,
            cx: &mut TaskContext<'_>,
            _: &mut [u8],
            _: Size,
        ) -> Poll<Result<usize>> {
            // This fake arranges an immediate external recheck; real devices use events.
            cx.waker().wake_by_ref();
            Poll::Pending
        }
        fn cancel_operation(&self, operation: Operation) {
            assert_eq!(operation, Operation::Read);
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn direct_poll_hooks_dispatch_through_dyn_base() {
        let device = EventDevice(AtomicUsize::new(0));
        let base: &dyn BaseOperations = &device;
        let mut context = Context::new_empty();
        let mut cx = TaskContext::from_waker(Waker::noop());
        assert!(base.supports_polling());
        assert!(
            base.poll_read(&mut context, &mut cx, &mut [0], 0)
                .is_pending()
        );
        base.cancel_operation(&mut context, Operation::Read);
        assert_eq!(device.0.load(Ordering::Relaxed), 1);
    }
}
