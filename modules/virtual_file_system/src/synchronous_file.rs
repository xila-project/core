use core::fmt::Debug;

use crate::{Error, ItemStatic, Result, VirtualFileSystem};
use alloc::{vec, vec::Vec};
use exported_file_system::StateFlags;
use exported_file_system::{ControlCommand, ControlCommandIdentifier, Permissions};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, Context, Flags, Path, Position, Size, Statistics,
};
use shared::AnyByLayout;
use task::TaskIdentifier;
use task::block_on;
use users::{GroupIdentifier, UserIdentifier};

pub struct SynchronousFile<'a> {
    closed: bool,
    virtual_file_system: &'a VirtualFileSystem,
    pub(crate) item: ItemStatic,
    cursor: crate::cursor::Cursor,
    pub(crate) flags: Flags,
    pub(crate) context: Context,
}

impl Debug for SynchronousFile<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SynchronousFile")
            .field("item", &self.item)
            .field(
                "position",
                &self.cursor.update(|position| Ok((position, position))),
            )
            .field("flags", &self.flags)
            .finish()
    }
}

impl<'a> SynchronousFile<'a> {
    pub(crate) const fn new(
        virtual_file_system: &'a VirtualFileSystem,
        item: ItemStatic,
        position: Size,
        flags: Flags,
        context: Context,
    ) -> Self {
        Self {
            closed: false,
            virtual_file_system,
            item,
            cursor: crate::cursor::Cursor::new(position),
            flags,
            context,
        }
    }

    pub fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        flags: Flags,
    ) -> Result<Self> {
        let file_identifier = block_on(virtual_file_system.open(&path, flags, task))?;

        Ok(file_identifier.into_synchronous_file())
    }

    pub fn set_position(&mut self, position: &Position) -> Result<Size> {
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.update(|current| {
            let next = operations.set_position(context, current, position)?;
            Ok((next, next))
        })
    }

    // - Operations

    pub(crate) fn cancel_operation(&mut self, operation: file_system::Operation) {
        if let Some(operations) = self.item.as_base_operations() {
            operations.cancel_operation(&mut self.context, operation);
        }
    }

    pub(crate) fn poll_read(
        &mut self,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return core::task::Poll::Ready(Err(Error::InvalidMode));
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.poll_update(|position| {
            operations
                .read(context, cx, buffer, position)
                .map(|result| {
                    result
                        .map(|count| (count, position + count as Size))
                        .map_err(Error::from)
                })
        })
    }

    pub(crate) fn poll_read_until(
        &mut self,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        delimiter: &[u8],
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return core::task::Poll::Ready(Err(Error::InvalidMode));
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.poll_update(|position| {
            operations
                .read_until(context, cx, buffer, position, delimiter)
                .map(|result| {
                    result
                        .map(|count| (count, position + count as Size))
                        .map_err(Error::from)
                })
        })
    }

    pub(crate) fn poll_write(
        &mut self,
        cx: &mut core::task::Context<'_>,
        buffer: &[u8],
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return core::task::Poll::Ready(Err(Error::InvalidMode));
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.poll_update(|position| {
            operations
                .write(context, cx, buffer, position)
                .map(|result| {
                    result
                        .map(|count| (count, position + count as Size))
                        .map_err(Error::from)
                })
        })
    }

    pub(crate) fn poll_flush(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<()>> {
        self.item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .flush(&mut self.context, cx)
            .map(|result| result.map_err(Error::from))
    }

    pub(crate) fn poll_control(
        &mut self,
        cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        self.item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .control(&mut self.context, cx, command, input, output)
            .map(|result| result.map_err(Error::from))
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(Error::InvalidMode);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.update(|position| {
            let count = operations.write_sync(context, buffer, position)?;
            Ok((count, position + count as Size))
        })
    }

    pub fn write_vectored(&mut self, buffers: &[&[u8]]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(Error::InvalidMode);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.update(|position| {
            let count = operations.write_vectored_sync(context, buffers, position)?;
            Ok((count, position + count as Size))
        })
    }

    pub fn write_line(&mut self, buffer: &[u8]) -> Result<usize> {
        let size = self.write(buffer)? + self.write(b"\n")?;

        Ok(size)
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return Err(Error::InvalidMode);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.update(|position| {
            let count = operations.read_sync(context, buffer, position)?;
            Ok((count, position + count as Size))
        })
    }

    pub fn read_until(&mut self, buffer: &mut [u8], delimiter: &[u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return Err(Error::InvalidMode);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?;
        let context = &mut self.context;
        self.cursor.update(|position| {
            let count = operations.read_until_sync(context, buffer, position, delimiter)?;
            Ok((count, position + count as Size))
        })
    }

    pub fn read_to_end(&mut self, buffer: &mut Vec<u8>, chunk_size: usize) -> Result<usize> {
        let mut total_read_size = 0;

        let mut chunk = vec![0u8; chunk_size];
        loop {
            let read_size = self.read(&mut chunk)?;

            if read_size == 0 {
                break;
            }

            total_read_size += read_size;

            buffer.extend_from_slice(&chunk[..read_size]);

            // Yield to allow other tasks to run.
            //yield_now().await;
        }

        Ok(total_read_size)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .flush_sync(&mut self.context)?;

        Ok(())
    }

    pub fn duplicate(&self) -> Result<Self> {
        let context = self
            .item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .clone_context(&self.context)?;
        block_on(self.virtual_file_system.retain(&self.item))?;

        Ok(Self {
            closed: false,
            virtual_file_system: self.virtual_file_system,
            item: self.item.clone(),
            cursor: self.cursor.duplicate(),
            flags: self.flags,
            context,
        })
    }

    pub fn get_statistics(&mut self) -> Result<Statistics> {
        let mut attributes = Attributes::new().set_mask(AttributeFlags::all());

        self.item
            .as_attributes_operations()
            .ok_or(Error::UnsupportedOperation)?
            .get_attributes(&mut self.context, &mut attributes)?;

        Statistics::from_attributes(&attributes).ok_or(Error::MissingAttribute)
    }

    pub fn set_owner(
        &mut self,
        user: Option<UserIdentifier>,
        group: Option<GroupIdentifier>,
    ) -> Result<()> {
        let mut attributes = Attributes::new();

        if let Some(user) = user {
            attributes = attributes.set_user(user);
        }

        if let Some(group) = group {
            attributes = attributes.set_group(group);
        }

        self.item
            .as_attributes_operations()
            .ok_or(Error::UnsupportedOperation)?
            .set_attributes(&mut self.context, &attributes)?;

        Ok(())
    }

    pub fn set_permissions(&mut self, permissions: Permissions) -> Result<()> {
        let attributes = Attributes::new().set_permissions(permissions);

        self.item
            .as_attributes_operations()
            .ok_or(Error::UnsupportedOperation)?
            .set_attributes(&mut self.context, &attributes)?;

        Ok(())
    }

    pub fn control<C>(&mut self, _command: C, argument: &C::Input) -> Result<C::Output>
    where
        C: ControlCommand,
        C::Output: Default,
    {
        let mut output = C::Output::default();

        self.item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .control_sync(
                &mut self.context,
                C::IDENTIFIER,
                AnyByLayout::from(argument),
                AnyByLayout::from_mutable(&mut output),
            )?;

        Ok(output)
    }

    /// Forwards an opaque control command and caller-owned payload layouts.
    pub fn control_raw(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        self.item
            .as_base_operations()
            .ok_or(Error::UnsupportedOperation)?
            .control_sync(&mut self.context, command, input, output)?;

        Ok(())
    }

    pub fn get_access(&self) -> Result<AccessFlags> {
        Ok(self.flags.get_access())
    }

    pub fn get_state(&self) -> Result<StateFlags> {
        Ok(self.flags.get_state())
    }

    pub fn close_internal(&mut self, virtual_file_system: &VirtualFileSystem) -> crate::Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        block_on(virtual_file_system.close(&self.item, &mut self.context))
    }

    pub fn close(mut self, virtual_file_system: &VirtualFileSystem) -> crate::Result<()> {
        self.close_internal(virtual_file_system)
    }
}

impl Drop for SynchronousFile<'_> {
    fn drop(&mut self) {
        // Note: We cannot use async in Drop, so we just ignore errors here.
        let _ = self.close_internal(self.virtual_file_system).map_err(|e| {
            log::warning!("Failed to close SynchronousFile in Drop: {e}");
        });
    }
}
