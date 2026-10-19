//! Borrowed operation state: dropping the future detaches device registrations.
use crate::{Result, SynchronousFile};
use core::future::poll_fn;
use file_system::{ControlCommand, Operation};
use shared::AnyByLayout;

struct Guard<'operation, 'vfs> {
    file: &'operation mut SynchronousFile<'vfs>,
    operation: Operation,
}
impl Drop for Guard<'_, '_> {
    fn drop(&mut self) {
        self.file.cancel_operation(self.operation);
    }
}

pub(crate) async fn read(file: &mut SynchronousFile<'_>, buffer: &mut [u8]) -> Result<usize> {
    let guard = Guard {
        file,
        operation: Operation::Read,
    };
    poll_fn(|cx| guard.file.poll_read(cx, buffer)).await
}

pub(crate) async fn read_until(
    file: &mut SynchronousFile<'_>,
    buffer: &mut [u8],
    delimiter: &[u8],
) -> Result<usize> {
    let guard = Guard {
        file,
        operation: Operation::Read,
    };
    poll_fn(|cx| guard.file.poll_read_until(cx, buffer, delimiter)).await
}

pub(crate) async fn write(file: &mut SynchronousFile<'_>, buffer: &[u8]) -> Result<usize> {
    let guard = Guard {
        file,
        operation: Operation::Write,
    };
    poll_fn(|cx| guard.file.poll_write(cx, buffer)).await
}

pub(crate) async fn flush(file: &mut SynchronousFile<'_>) -> Result<()> {
    let guard = Guard {
        file,
        operation: Operation::Flush,
    };
    poll_fn(|cx| guard.file.poll_flush(cx)).await
}

pub(crate) async fn control<C>(
    file: &mut SynchronousFile<'_>,
    _command: C,
    argument: &C::Input,
) -> Result<C::Output>
where
    C: ControlCommand,
    C::Output: Default,
{
    let guard = Guard {
        file,
        operation: Operation::Control,
    };
    let mut output = C::Output::default();
    poll_fn(|cx| {
        guard.file.poll_control(
            cx,
            C::IDENTIFIER,
            AnyByLayout::from(argument),
            AnyByLayout::from_mutable(&mut output),
        )
    })
    .await?;
    Ok(output)
}
