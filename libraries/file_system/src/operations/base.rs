//! One object-safe polling contract for device I/O.
use super::polling::Operation;
use crate::{Context, ControlCommandIdentifier, Error, Position, Result, Size};
use core::task::{Context as TaskContext, Poll, Waker};
use shared::AnyByLayout;

/// Complete an intrinsically synchronous backend operation with the polling contract.
/// The closure runs exactly once; busy errors remain errors.
pub fn ready<T>(operation: impl FnOnce() -> Result<T>) -> Poll<Result<T>> {
    Poll::Ready(operation())
}

fn immediate<T>(result: Poll<Result<T>>) -> Result<T> {
    match result {
        Poll::Ready(result) => result,
        Poll::Pending => Err(Error::RessourceBusy),
    }
}

/// Device operations return `Poll` directly, including `read`. `Pending` requires
/// race-free waker registration and must not hide consumed bytes. Never retain
/// caller buffers or task-context pointers. Device-owned state belongs in the
/// opening context (or the device for context-free implementations).
///
/// `_sync` methods are explicit single-attempt adapters for synchronous C/OS
/// boundaries. They cancel registrations and return busy on Pending, never retry.
pub trait BaseOperations: Send + Sync {
    fn read(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>>;
    fn write(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>>;
    fn flush(&self, _context: &mut Context, _cx: &mut TaskContext<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn control(
        &self,
        _context: &mut Context,
        _cx: &mut TaskContext<'_>,
        _command: ControlCommandIdentifier,
        _input: &AnyByLayout,
        _output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }
    fn read_until(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
        delimiter: &[u8],
    ) -> Poll<Result<usize>> {
        if delimiter.is_empty() {
            return Poll::Ready(Err(Error::InvalidParameter));
        }
        let mut total = 0;
        while total < buffer.len() {
            match self.read(
                context,
                cx,
                &mut buffer[total..total + 1],
                absolute_position + total as Size,
            ) {
                Poll::Ready(Ok(0)) => break,
                Poll::Ready(Ok(n)) => total += n,
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
            if buffer[..total].ends_with(delimiter) {
                break;
            }
        }
        Poll::Ready(Ok(total))
    }
    fn write_pattern(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        let mut total = 0;
        for buffer in core::iter::repeat_n(pattern, count) {
            if buffer.is_empty() {
                continue;
            }
            match self.write(context, cx, buffer, absolute_position + total as Size) {
                Poll::Ready(Ok(n)) => {
                    total += n;
                    if n < buffer.len() {
                        break;
                    }
                }
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
        }
        Poll::Ready(Ok(total))
    }
    fn write_vectored(
        &self,
        context: &mut Context,
        cx: &mut TaskContext<'_>,
        buffers: &[&[u8]],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        let mut total = 0;
        for buffer in buffers.iter().copied() {
            if buffer.is_empty() {
                continue;
            }
            match self.write(context, cx, buffer, absolute_position + total as Size) {
                Poll::Ready(Ok(n)) => {
                    total += n;
                    if n < buffer.len() {
                        break;
                    }
                }
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
        }
        Poll::Ready(Ok(total))
    }
    fn read_sync(
        &self,
        context: &mut Context,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.read(context, &mut cx, buffer, absolute_position);
        self.cancel_operation(context, Operation::Read);
        immediate(result)
    }
    fn write_sync(
        &self,
        context: &mut Context,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write(context, &mut cx, buffer, absolute_position);
        self.cancel_operation(context, Operation::Write);
        immediate(result)
    }
    fn flush_sync(&self, context: &mut Context) -> Result<()> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.flush(context, &mut cx);
        self.cancel_operation(context, Operation::Flush);
        immediate(result)
    }
    fn control_sync(
        &self,
        context: &mut Context,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.control(context, &mut cx, command, input, output);
        self.cancel_operation(context, Operation::Control);
        immediate(result)
    }
    fn read_until_sync(
        &self,
        context: &mut Context,
        buffer: &mut [u8],
        absolute_position: Size,
        delimiter: &[u8],
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.read_until(context, &mut cx, buffer, absolute_position, delimiter);
        self.cancel_operation(context, Operation::Read);
        immediate(result)
    }
    fn write_pattern_sync(
        &self,
        context: &mut Context,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write_pattern(context, &mut cx, pattern, count, absolute_position);
        self.cancel_operation(context, Operation::Write);
        immediate(result)
    }
    fn write_vectored_sync(
        &self,
        context: &mut Context,
        buffers: &[&[u8]],
        absolute_position: Size,
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write_vectored(context, &mut cx, buffers, absolute_position);
        self.cancel_operation(context, Operation::Write);
        immediate(result)
    }
    /// Detach this opening's registration after completion, error or cancellation.
    /// This hook is idempotent and must not consume data or change the cursor.
    /// Wake and drop retired wakers outside device locks.
    fn cancel_operation(&self, _context: &mut Context, _operation: Operation) {}
    fn open(&self, _context: &mut Context) -> Result<()> {
        Ok(())
    }
    fn close(&self, _context: &mut Context) -> Result<()> {
        Ok(())
    }
    fn set_position(
        &self,
        _context: &mut Context,
        current: Size,
        position: &Position,
    ) -> Result<Size> {
        match position {
            Position::Start(value) => Ok(*value),
            Position::Current(offset) => Ok(current.wrapping_add(*offset as Size)),
            Position::End(_) => Err(Error::UnsupportedOperation),
        }
    }
    fn clone_context(&self, context: &Context) -> Result<Context>;
}

/// Device operations return `Poll` directly, including `read`. `Pending` requires
/// race-free waker registration and must not hide consumed bytes. Never retain
/// caller buffers or task-context pointers. Device-owned state belongs in the
/// opening context (or the device for context-free implementations).
///
/// `_sync` methods are explicit single-attempt adapters for synchronous C/OS
/// boundaries. They cancel registrations and return busy on Pending, never retry.
pub trait DirectBaseOperations: Send + Sync {
    fn read(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>>;
    fn write(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>>;
    fn flush(&self, _cx: &mut TaskContext<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn control(
        &self,
        _cx: &mut TaskContext<'_>,
        _command: ControlCommandIdentifier,
        _input: &AnyByLayout,
        _output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }
    fn read_until(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
        delimiter: &[u8],
    ) -> Poll<Result<usize>> {
        if delimiter.is_empty() {
            return Poll::Ready(Err(Error::InvalidParameter));
        }
        let mut total = 0;
        while total < buffer.len() {
            match self.read(
                cx,
                &mut buffer[total..total + 1],
                absolute_position + total as Size,
            ) {
                Poll::Ready(Ok(0)) => break,
                Poll::Ready(Ok(n)) => total += n,
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
            if buffer[..total].ends_with(delimiter) {
                break;
            }
        }
        Poll::Ready(Ok(total))
    }
    fn write_pattern(
        &self,
        cx: &mut TaskContext<'_>,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        let mut total = 0;
        for buffer in core::iter::repeat_n(pattern, count) {
            if buffer.is_empty() {
                continue;
            }
            match self.write(cx, buffer, absolute_position + total as Size) {
                Poll::Ready(Ok(n)) => {
                    total += n;
                    if n < buffer.len() {
                        break;
                    }
                }
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
        }
        Poll::Ready(Ok(total))
    }
    fn write_vectored(
        &self,
        cx: &mut TaskContext<'_>,
        buffers: &[&[u8]],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        let mut total = 0;
        for buffer in buffers.iter().copied() {
            if buffer.is_empty() {
                continue;
            }
            match self.write(cx, buffer, absolute_position + total as Size) {
                Poll::Ready(Ok(n)) => {
                    total += n;
                    if n < buffer.len() {
                        break;
                    }
                }
                Poll::Pending if total == 0 => return Poll::Pending,
                Poll::Ready(Err(error)) if total == 0 => return Poll::Ready(Err(error)),
                _ => break,
            }
        }
        Poll::Ready(Ok(total))
    }
    fn read_sync(&self, buffer: &mut [u8], absolute_position: Size) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.read(&mut cx, buffer, absolute_position);
        self.cancel_operation(Operation::Read);
        immediate(result)
    }
    fn write_sync(&self, buffer: &[u8], absolute_position: Size) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write(&mut cx, buffer, absolute_position);
        self.cancel_operation(Operation::Write);
        immediate(result)
    }
    fn flush_sync(&self) -> Result<()> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.flush(&mut cx);
        self.cancel_operation(Operation::Flush);
        immediate(result)
    }
    fn control_sync(
        &self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.control(&mut cx, command, input, output);
        self.cancel_operation(Operation::Control);
        immediate(result)
    }
    fn read_until_sync(
        &self,
        buffer: &mut [u8],
        absolute_position: Size,
        delimiter: &[u8],
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.read_until(&mut cx, buffer, absolute_position, delimiter);
        self.cancel_operation(Operation::Read);
        immediate(result)
    }
    fn write_pattern_sync(
        &self,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write_pattern(&mut cx, pattern, count, absolute_position);
        self.cancel_operation(Operation::Write);
        immediate(result)
    }
    fn write_vectored_sync(&self, buffers: &[&[u8]], absolute_position: Size) -> Result<usize> {
        let mut cx = TaskContext::from_waker(Waker::noop());
        let result = self.write_vectored(&mut cx, buffers, absolute_position);
        self.cancel_operation(Operation::Write);
        immediate(result)
    }
    /// Detach the current operation's registration; see `BaseOperations`.
    fn cancel_operation(&self, _operation: Operation) {}
    fn open(&self) -> Result<()> {
        Ok(())
    }
    fn close(&self) -> Result<()> {
        Ok(())
    }
    fn set_position(&self, current: Size, position: &Position) -> Result<Size> {
        match position {
            Position::Start(value) => Ok(*value),
            Position::Current(offset) => Ok(current.wrapping_add(*offset as Size)),
            Position::End(_) => Err(Error::UnsupportedOperation),
        }
    }
}

impl<T: DirectBaseOperations> BaseOperations for T {
    fn read(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        DirectBaseOperations::read(self, cx, buffer, absolute_position)
    }
    fn write(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        DirectBaseOperations::write(self, cx, buffer, absolute_position)
    }
    fn flush(&self, _: &mut Context, cx: &mut TaskContext<'_>) -> Poll<Result<()>> {
        DirectBaseOperations::flush(self, cx)
    }
    fn control(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        DirectBaseOperations::control(self, cx, command, input, output)
    }
    fn read_until(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
        delimiter: &[u8],
    ) -> Poll<Result<usize>> {
        DirectBaseOperations::read_until(self, cx, buffer, absolute_position, delimiter)
    }
    fn write_pattern(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        DirectBaseOperations::write_pattern(self, cx, pattern, count, absolute_position)
    }
    fn write_vectored(
        &self,
        _: &mut Context,
        cx: &mut TaskContext<'_>,
        buffers: &[&[u8]],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        DirectBaseOperations::write_vectored(self, cx, buffers, absolute_position)
    }
    fn open(&self, _: &mut Context) -> Result<()> {
        DirectBaseOperations::open(self)
    }
    fn close(&self, _: &mut Context) -> Result<()> {
        DirectBaseOperations::close(self)
    }
    fn set_position(&self, _: &mut Context, current: Size, position: &Position) -> Result<Size> {
        DirectBaseOperations::set_position(self, current, position)
    }
    fn clone_context(&self, _: &Context) -> Result<Context> {
        Ok(Context::new_empty())
    }
    fn cancel_operation(&self, _: &mut Context, operation: Operation) {
        DirectBaseOperations::cancel_operation(self, operation)
    }
}
pub fn open_close_operation<D: DirectBaseOperations, R>(
    device: &D,
    operation: impl Fn(&D) -> Result<R>,
) -> Result<R> {
    device.open()?;
    let result = operation(device);
    device.close()?;
    result
}
