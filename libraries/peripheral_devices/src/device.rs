//! Generic character-device wrapper around a peripheral handle.

use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
};

use alloc::boxed::Box;
use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommandIdentifier, Error, MountOperations,
    Result, Size, ready,
};
use shared::AnyByLayout;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

/// Class-specific handle wrapped by [`PeripheralDevice`].
///
/// Every method is a single synchronous attempt. It must return promptly
/// without waiting for hardware, because it runs on the executor. A peripheral
/// that would have to wait returns `Error::RessourceBusy` (for example when a
/// UART reports `WouldBlock`), and the caller decides whether to retry. Callers
/// do not retain buffers past the call.
///
/// `control` forwards command identifiers that the class defined itself. The
/// base never interprets them.
pub trait PeripheralHandle: Send {
    /// Read available bytes into `buffer`. `Ok(0)` means end of stream.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize>;

    /// Write bytes from `buffer` and return the number accepted.
    fn write(&mut self, buffer: &[u8]) -> Result<usize>;

    /// Push queued output without waiting for the device to drain, if supported.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Class-defined control. The default rejects every command.
    fn control(
        &mut self,
        _command: ControlCommandIdentifier,
        _input: &AnyByLayout,
        _output: &mut AnyByLayout,
    ) -> Result<()> {
        Err(Error::UnsupportedOperation)
    }
}

/// Marker stored in an opening's `Context`. Only the context that acquired the
/// device carries it, so closing a cloned context cannot release the device.
struct Opening;

/// Character device exposing a [`PeripheralHandle`] through the VFS polling contract.
///
/// Opening is exclusive: while one opening is held, another `open` returns
/// `Error::RessourceBusy`. The exclusivity is released by `close` on the
/// opening's own context. A context created by `clone_context` does not own
/// the device. Read, write, flush and control never return `Pending`; they run
/// once through `ready` and return `RessourceBusy` if the handle is in use.
/// Reads and writes ignore the absolute position because the device is a stream.
pub struct PeripheralDevice<H> {
    opened: AtomicBool,
    handle: Mutex<CriticalSectionRawMutex, H>,
}

impl<H: PeripheralHandle> PeripheralDevice<H> {
    pub const fn new(handle: H) -> Self {
        Self {
            opened: AtomicBool::new(false),
            handle: Mutex::new(handle),
        }
    }

    /// Runs `operation` on the handle once, without waiting for the lock.
    fn with_handle<R>(&self, operation: impl FnOnce(&mut H) -> Result<R>) -> Result<R> {
        let mut handle = self.handle.try_lock().map_err(|_| Error::RessourceBusy)?;
        operation(&mut handle)
    }
}

impl<H: PeripheralHandle> BaseOperations for PeripheralDevice<H> {
    fn open(&self, context: &mut Context) -> Result<()> {
        self.opened
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::RessourceBusy)?;
        context.set_private_data(Box::new(Opening));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        if context.take_private_data_of_type::<Opening>().is_some() {
            self.opened.store(false, Ordering::Release);
        }
        Ok(())
    }

    fn read(
        &self,
        _: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        ready(|| self.with_handle(|handle| handle.read(buffer)))
    }

    fn write(
        &self,
        _: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        ready(|| self.with_handle(|handle| handle.write(buffer)))
    }

    fn flush(&self, _: &mut Context, _cx: &mut core::task::Context<'_>) -> Poll<Result<()>> {
        ready(|| self.with_handle(|handle| handle.flush()))
    }

    fn control(
        &self,
        _: &mut Context,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        ready(|| self.with_handle(|handle| handle.control(command, input, output)))
    }

    fn clone_context(&self, _: &Context) -> Result<Context> {
        Ok(Context::new_empty())
    }
}

impl<H: PeripheralHandle> MountOperations for PeripheralDevice<H> {}

impl<H: PeripheralHandle> CharacterDevice for PeripheralDevice<H> {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::Waker;

    #[derive(Default)]
    struct Sink {
        written: usize,
    }

    impl PeripheralHandle for Sink {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
            buffer.fill(0);
            Ok(buffer.len())
        }

        fn write(&mut self, buffer: &[u8]) -> Result<usize> {
            self.written += buffer.len();
            Ok(buffer.len())
        }
    }

    #[test]
    fn open_is_exclusive_and_only_the_owning_context_releases_it() {
        let device = PeripheralDevice::new(Sink::default());
        let mut owner = Context::new_empty();
        let mut clone = device.clone_context(&owner).unwrap();

        device.open(&mut owner).unwrap();
        assert_eq!(
            device.open(&mut Context::new_empty()),
            Err(Error::RessourceBusy)
        );

        // Closing a context that does not own the opening keeps the device held.
        device.close(&mut clone).unwrap();
        assert_eq!(
            device.open(&mut Context::new_empty()),
            Err(Error::RessourceBusy)
        );

        device.close(&mut owner).unwrap();
        device.open(&mut Context::new_empty()).unwrap();
    }

    #[test]
    fn read_and_write_run_once_and_never_pend() {
        let device = PeripheralDevice::new(Sink::default());
        let mut context = Context::new_empty();
        let mut cx = core::task::Context::from_waker(Waker::noop());
        let mut buffer = [1u8; 3];

        assert_eq!(
            device.read(&mut context, &mut cx, &mut buffer, 0),
            Poll::Ready(Ok(3))
        );
        assert_eq!(buffer, [0; 3]);
        assert_eq!(
            device.write(&mut context, &mut cx, b"abcd", 0),
            Poll::Ready(Ok(4))
        );
        assert_eq!(device.with_handle(|handle| Ok(handle.written)), Ok(4));
    }
}
