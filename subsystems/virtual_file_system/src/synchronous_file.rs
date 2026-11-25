use core::fmt::Debug;

use crate::{
    IoErrorKind, ItemStatic, Result, VirtualFileSystem, suspend_on, suspend_on_cancellable,
};
use alloc::{vec, vec::Vec};
use exported_file_system::StateFlags;
use exported_file_system::{ControlCommand, ControlCommandIdentifier, Permissions};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, Context, Flags, Inode, Path, Position, Size,
    Statistics,
};
use shared::AnyByLayout;
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};

pub struct SynchronousFile<'a> {
    closed: bool,
    // Native callers are trusted; this is the retained task context of the
    // opening, not a sandbox between native components.
    pub(crate) authority: Option<task::TaskAuthority<'static>>,
    virtual_file_system: &'a VirtualFileSystem,
    pub(crate) item: ItemStatic,
    /// Inode of the device entry the opening took its reference on. Two
    /// mounted devices can share a pointer (every value of a zero-sized type
    /// does), so references are released by inode, not by pointer.
    pub(crate) device_inode: Option<Inode>,
    cursor: crate::cursor::Cursor,
    pub(crate) flags: Flags,
    pub(crate) context: Context,
    /// The modification and status times were recorded on this handle since
    /// the last flush: only the first write of more than zero bytes after an
    /// open or a flush touches them, so repeated writes cost no extra metadata.
    times_touched: bool,
    /// A metadata-update failure was already logged for this handle.
    time_error_logged: bool,
}

impl Debug for SynchronousFile<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SynchronousFile")
            .field("item", &self.item)
            .field("position", &self.cursor.position())
            .field("flags", &self.flags.set_status(self.state()))
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
            authority: None,
            virtual_file_system,
            item,
            device_inode: None,
            cursor: crate::cursor::Cursor::new(position, flags.get_state()),
            flags: flags.set_status(StateFlags::empty()),
            context,
            times_touched: false,
            time_error_logged: false,
        }
    }

    /// Open a file. In a stackful coroutine the call suspends, and a cancellation drops it
    /// in place: the wait owns the path and the namespace lock it may hold, so neither is
    /// left behind in the abandoned frames. An open the cancellation interrupts may have
    /// created the file already.
    pub fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        flags: Flags,
    ) -> Result<Self> {
        let file_identifier = suspend_on_cancellable(async move {
            virtual_file_system.open(&path.as_ref(), flags, task).await
        })?;

        Ok(file_identifier.into_synchronous_file())
    }

    pub fn set_position(&mut self, position: &Position) -> Result<Size> {
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        self.cursor.update(|current| {
            let absolute;
            let position = if let Position::Current(delta) = position {
                absolute = Position::Start(
                    current
                        .checked_add_signed(*delta)
                        .ok_or(IoErrorKind::InvalidArgument)?,
                );
                &absolute
            } else {
                position
            };
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
        offset: Option<Size>,
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return core::task::Poll::Ready(Err(IoErrorKind::BadDescriptor));
        }
        if let Some(offset) = offset {
            let operations = Self::positional_operations(&self.item)?;
            return operations.read(&mut self.context, cx, buffer, offset);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        self.cursor.poll_update(|position| {
            operations
                .read(context, cx, buffer, position)
                .map(|result| result.map(|count| (count, position + count as Size)))
        })
    }

    pub(crate) fn poll_read_until(
        &mut self,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        delimiter: &[u8],
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return core::task::Poll::Ready(Err(IoErrorKind::BadDescriptor));
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        self.cursor.poll_update(|position| {
            operations
                .read_until(context, cx, buffer, position, delimiter)
                .map(|result| result.map(|count| (count, position + count as Size)))
        })
    }

    fn file_system_operations(&self) -> Option<&'static dyn file_system::FileSystemOperations> {
        match &self.item {
            ItemStatic::File(file_system) => Some(*file_system),
            _ => None,
        }
    }

    /// Update the modification and status times after a mutation of an open
    /// regular file, once per open or flush interval. The backend commits them
    /// with the file data on the next sync or close, so this costs no extra
    /// flash write. Best effort: the triggering operation already succeeded.
    fn touch_modification_time(&mut self) {
        if self.times_touched {
            return;
        }
        if let Err(error) = self
            .virtual_file_system
            .touch_handle_modification_time(&self.item, &mut self.context)
        {
            match error {
                IoErrorKind::Unsupported | IoErrorKind::ResourceBusy | IoErrorKind::WouldBlock => {}
                _ => {
                    // Log once per handle, not on every write of an errored one.
                    if !self.time_error_logged {
                        self.time_error_logged = true;
                        log::warning!("Failed to update modification time after a write: {error}");
                    }
                }
            }
        } else {
            self.times_touched = true;
        }
    }

    /// Attach the result of a backend write to the modification-time update.
    fn after_write(
        &mut self,
        result: core::task::Poll<Result<usize>>,
    ) -> core::task::Poll<Result<usize>> {
        if let core::task::Poll::Ready(Ok(count)) = result
            && count > 0
        {
            self.touch_modification_time();
        }
        result
    }

    fn append_position(&mut self) -> Option<Size> {
        // Some devices cannot report a size. Keep the cursor for those devices.
        if self.state().contains(StateFlags::Append) {
            self.get_statistics().ok().map(|statistics| statistics.size)
        } else {
            None
        }
    }

    pub(crate) fn poll_write(
        &mut self,
        cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        offset: Option<Size>,
    ) -> core::task::Poll<Result<usize>> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return core::task::Poll::Ready(Err(IoErrorKind::BadDescriptor));
        }
        if let Some(offset) = offset {
            let result = Self::positional_operations(&self.item)?.write(
                &mut self.context,
                cx,
                buffer,
                offset,
            );
            return self.after_write(result);
        }
        let append_position = self.append_position();
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        let result = self.cursor.poll_update(|position| {
            let position = append_position.unwrap_or(position);
            operations
                .write(context, cx, buffer, position)
                .map(|result| result.map(|count| (count, position + count as Size)))
        });
        self.after_write(result)
    }

    pub(crate) fn poll_flush(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<()>> {
        let result = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .flush(&mut self.context, cx);
        // A successful flush committed the pending data and times: the next
        // write is a new mutation and records the times again.
        if let core::task::Poll::Ready(Ok(())) = result {
            self.times_touched = false;
        }
        result
    }

    pub(crate) fn poll_control(
        &mut self,
        cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        if command
            .get_direction()
            .contains(file_system::ControlDirectionFlags::Write)
            && !self.flags.get_access().contains(AccessFlags::Write)
        {
            return core::task::Poll::Ready(Err(IoErrorKind::BadDescriptor));
        }
        self.item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .control(&mut self.context, cx, command, input, output)
    }

    pub fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let append_position = self.append_position();
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        let result = self.cursor.update(|position| {
            let position = append_position.unwrap_or(position);
            let count = operations.write_sync(context, buffer, position)?;
            Ok((count, position + count as Size))
        });
        if let Ok(count) = result
            && count > 0
        {
            self.touch_modification_time();
        }
        result
    }

    pub fn write_vectored(&mut self, buffers: &[&[u8]]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        let result = self.cursor.update(|position| {
            let count = operations.write_vectored_sync(context, buffers, position)?;
            Ok((count, position + count as Size))
        });
        if let Ok(count) = result
            && count > 0
        {
            self.touch_modification_time();
        }
        result
    }

    pub fn write_line(&mut self, buffer: &[u8]) -> Result<usize> {
        let size = self.write(buffer)? + self.write(b"\n")?;

        Ok(size)
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
        let context = &mut self.context;
        self.cursor.update(|position| {
            let count = operations.read_sync(context, buffer, position)?;
            Ok((count, position + count as Size))
        })
    }

    pub fn read_until(&mut self, buffer: &mut [u8], delimiter: &[u8]) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let operations = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?;
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
            .ok_or(IoErrorKind::Unsupported)?
            .flush_sync(&mut self.context)?;
        // The flush committed the pending data and times: the next write is a
        // new mutation and records the times again.
        self.times_touched = false;

        Ok(())
    }

    pub fn duplicate(&self) -> Result<Self> {
        let context = self
            .item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .clone_context(&self.context)?;
        suspend_on(
            self.virtual_file_system
                .retain(&self.item, self.device_inode),
        )?;

        Ok(Self {
            closed: false,
            authority: self.authority,
            virtual_file_system: self.virtual_file_system,
            item: self.item.clone(),
            device_inode: self.device_inode,
            cursor: self.cursor.duplicate(),
            flags: self.flags,
            context,
            // The duplicate shares the native context, whose pending times
            // are already recorded.
            times_touched: self.times_touched,
            time_error_logged: self.time_error_logged,
        })
    }

    pub fn get_statistics(&mut self) -> Result<Statistics> {
        let mut attributes = Attributes::new().set_mask(AttributeFlags::all());

        self.item
            .as_attributes_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .get_attributes(&mut self.context, &mut attributes)?;

        Statistics::from_attributes(&attributes).ok_or(IoErrorKind::InputOutput)
    }

    /// Resize through an already opened writable file without changing its cursor.
    /// Resizing mutates the file, so the modification and status times are
    /// updated with the size change and committed together with it.
    pub fn set_size(&mut self, size: Size) -> Result<()> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let mut attributes = Attributes::new().set_size(size);
        if let Some(file_system) = self.file_system_operations()
            && let Ok(time) = self.virtual_file_system.current_time()
        {
            let times = Attributes::new().set_modification(time).set_status(time);
            let mask = AttributeFlags::Size
                | times
                    .get_mask()
                    .intersection(file_system.writable_attributes());
            attributes = attributes
                .set_modification(time)
                .set_status(time)
                .set_mask(mask);
        }
        self.item
            .as_attributes_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .set_attributes(&mut self.context, &attributes)?;
        Ok(())
    }

    /// Update timestamps through retained handle authority.
    pub fn set_times(
        &mut self,
        access: Option<file_system::Time>,
        modification: Option<file_system::Time>,
    ) -> Result<()> {
        let mut attributes = Attributes::new();
        if let Some(time) = access {
            attributes = attributes.set_access(time);
        }
        if let Some(time) = modification {
            attributes = attributes.set_modification(time);
        }
        self.set_metadata(&attributes)
    }

    fn positional_operations(item: &ItemStatic) -> Result<&dyn file_system::BaseOperations> {
        if matches!(item, ItemStatic::Pipe(_) | ItemStatic::CharacterDevice(_)) {
            return Err(IoErrorKind::InvalidSeek);
        }
        item.as_base_operations().ok_or(IoErrorKind::Unsupported)
    }

    /// Read at an explicit offset, leaving the shared cursor unchanged.
    pub fn read_at(&mut self, buffer: &mut [u8], offset: Size) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Read) {
            return Err(IoErrorKind::BadDescriptor);
        }
        Self::positional_operations(&self.item)?.read_sync(&mut self.context, buffer, offset)
    }

    /// Write at an explicit offset, leaving the shared cursor unchanged.
    pub fn write_at(&mut self, buffer: &[u8], offset: Size) -> Result<usize> {
        if !self.flags.get_access().contains(AccessFlags::Write) {
            return Err(IoErrorKind::BadDescriptor);
        }
        let result =
            Self::positional_operations(&self.item)?.write_sync(&mut self.context, buffer, offset);
        if let Ok(count) = result
            && count > 0
        {
            self.touch_modification_time();
        }
        result
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

        self.set_metadata(&attributes)?;

        Ok(())
    }

    pub fn set_permissions(&mut self, permissions: Permissions) -> Result<()> {
        let attributes = Attributes::new().set_permissions(permissions);

        self.set_metadata(&attributes)?;

        Ok(())
    }

    fn set_metadata(&mut self, attributes: &Attributes) -> Result<()> {
        if self.closed {
            return Err(IoErrorKind::BadDescriptor);
        }
        self.virtual_file_system.set_handle_attributes(
            self.authority
                .as_ref()
                .ok_or(IoErrorKind::PermissionDenied)?,
            &self.item,
            &mut self.context,
            attributes,
        )
    }

    pub fn control<C>(&mut self, _command: C, argument: &C::Input) -> Result<C::Output>
    where
        C: ControlCommand,
        C::Output: Default,
    {
        let mut output = C::Output::default();

        if C::IDENTIFIER
            .get_direction()
            .contains(file_system::ControlDirectionFlags::Write)
            && !self.flags.get_access().contains(AccessFlags::Write)
        {
            return Err(IoErrorKind::BadDescriptor);
        }
        self.item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?
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
        if command
            .get_direction()
            .contains(file_system::ControlDirectionFlags::Write)
            && !self.flags.get_access().contains(AccessFlags::Write)
        {
            return Err(IoErrorKind::BadDescriptor);
        }
        self.item
            .as_base_operations()
            .ok_or(IoErrorKind::Unsupported)?
            .control_sync(&mut self.context, command, input, output)?;

        Ok(())
    }

    pub fn get_access(&self) -> Result<AccessFlags> {
        Ok(self.flags.get_access())
    }

    /// Set status flags on this open file description, including its duplicates.
    pub fn set_state(&mut self, state: StateFlags) -> Result<()> {
        if state.intersects(StateFlags::Synchronous | StateFlags::SynchronousDataOnly) {
            return Err(IoErrorKind::Unsupported);
        }
        self.cursor.set_state(state);
        Ok(())
    }

    pub(crate) fn state(&self) -> StateFlags {
        self.cursor.state()
    }

    pub fn get_state(&self) -> Result<StateFlags> {
        Ok(self.state())
    }

    pub fn close_internal(&mut self, virtual_file_system: &VirtualFileSystem) -> crate::Result<()> {
        if self.closed {
            return Ok(());
        }
        let result =
            suspend_on(virtual_file_system.close(&self.item, self.device_inode, &mut self.context));
        // Marked only once the close has run. A coroutine cancelled while the close waits
        // for the file system never gets here, and whoever owns the file next, which is the
        // native host closing what a cancelled process held, closes it instead of skipping it.
        self.closed = true;
        result
    }

    /// Close using the VFS retained by this file, without consuming it, so that its owner
    /// can keep it where a cancellation can still reach it until the close is done. Closing
    /// again, or dropping, does nothing more once this has returned.
    pub fn close_in_place(&mut self) -> crate::Result<()> {
        self.close_internal(self.virtual_file_system)
    }

    /// Close using the VFS retained by this file, reporting any close error.
    pub fn close_owned(mut self) -> crate::Result<()> {
        self.close_internal(self.virtual_file_system)
    }

    pub fn close(mut self, virtual_file_system: &VirtualFileSystem) -> crate::Result<()> {
        self.close_internal(virtual_file_system)
    }
}

impl Drop for SynchronousFile<'_> {
    /// Closes a file that was not closed, without waiting: a drop must never suspend, or a
    /// cancellation could abandon it halfway (see
    /// [`VirtualFileSystem::close_without_waiting`]). Errors are only logged.
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self
            .virtual_file_system
            .close_without_waiting(&self.item, self.device_inode, &mut self.context)
            .map_err(|e| {
                log::warning!("Failed to close SynchronousFile in Drop: {e}");
            });
    }
}
