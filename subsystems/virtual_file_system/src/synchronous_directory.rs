use core::mem::forget;
use exported_file_system::{
    AccessFlags, AttributeFlags, AttributeOperations, Attributes, StateFlags, Statistics,
};
use file_system::{Context, DirectoryOperations, Entry, Flags, Path, Size};
use task::TaskIdentifier;

use crate::{
    IoErrorKind, ItemStatic, Result, VirtualFileSystem, blocking_operation, suspend_on,
    suspend_on_cancellable,
};

pub struct SynchronousDirectory<'a> {
    closed: bool,
    virtual_file_system: &'a VirtualFileSystem,
    pub(crate) directory: &'static dyn file_system::FileSystemOperations,
    pub(crate) flags: Flags,
    pub(crate) context: Context,
}

impl<'a> SynchronousDirectory<'a> {
    pub fn new(
        virtual_file_system: &'a VirtualFileSystem,
        directory: &'static dyn file_system::FileSystemOperations,
        flags: Flags,
        context: Context,
    ) -> Self {
        Self {
            closed: false,
            virtual_file_system,
            directory,
            flags,
            context,
        }
    }

    fn blocking_operation<O>(
        &mut self,
        mut operation: impl FnMut(&mut Self) -> Result<O>,
    ) -> Result<O> {
        blocking_operation(self.flags, || operation(self))
    }

    pub fn create(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        suspend_on_cancellable(async move {
            virtual_file_system
                .create_directory(task, &path.as_ref())
                .await
        })
    }

    /// Open a directory. As for [`crate::SynchronousFile::open`], a cancellation in a
    /// stackful coroutine drops the wait, the path it owns and the lock it may hold.
    pub fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<Self> {
        let mut directory = suspend_on_cancellable(async move {
            virtual_file_system
                .open_directory(task, &path.as_ref())
                .await
        })?
        .into_synchronous_directory();

        directory.flags = directory.flags.set_status(StateFlags::empty());

        Ok(directory)
    }

    pub fn read(&mut self) -> Result<Option<Entry>> {
        let entry = self.blocking_operation(|directory| {
            DirectoryOperations::read(directory.directory, &mut directory.context)
        })?;

        Ok(entry)
    }

    pub fn get_statistics(&mut self) -> Result<Statistics> {
        self.blocking_operation(|directory| {
            let mut attributes = Attributes::new().set_mask(AttributeFlags::all());

            AttributeOperations::get_attributes(
                directory.directory,
                &mut directory.context,
                &mut attributes,
            )?;

            Statistics::from_attributes(&attributes).ok_or(IoErrorKind::InputOutput)
        })
    }

    pub fn get_position(&mut self) -> Result<Size> {
        self.blocking_operation(|directory| {
            DirectoryOperations::get_position(directory.directory, &mut directory.context)
        })
    }

    pub fn set_position(&mut self, position: Size) -> Result<()> {
        self.blocking_operation(|directory| {
            DirectoryOperations::set_position(directory.directory, &mut directory.context, position)
        })?;

        Ok(())
    }

    pub fn rewind(&mut self) -> Result<()> {
        self.blocking_operation(|directory| {
            DirectoryOperations::rewind(directory.directory, &mut directory.context)
        })
    }

    pub fn get_access(&self) -> Result<AccessFlags> {
        Ok(self.flags.get_access())
    }

    pub fn get_state(&self) -> Result<StateFlags> {
        Ok(self.flags.get_state())
    }

    pub fn close_internal(&mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let result = suspend_on(virtual_file_system.close(
            &ItemStatic::Directory(self.directory),
            None,
            &mut self.context,
        ));
        // Marked only once the close has run, as for a file: a close that is cancelled
        // while it waits is closed again by the next owner, not skipped.
        self.closed = true;
        result
    }

    /// Close using the retained VFS without consuming the listing, so that its owner can
    /// keep it where a cancellation can still reach it until the close is done. Closing
    /// again, or dropping, does nothing more once this has returned.
    pub fn close_in_place(&mut self) -> Result<()> {
        self.close_internal(self.virtual_file_system)
    }

    /// Close using the retained VFS, reporting any close error.
    pub fn close_owned(self) -> Result<()> {
        let virtual_file_system = self.virtual_file_system;
        self.close(virtual_file_system)
    }

    pub fn close(mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        let result = self.close_internal(virtual_file_system);
        forget(self);

        result
    }
}

impl Drop for SynchronousDirectory<'_> {
    /// Closes a listing that was not closed, without waiting, as a file's drop does.
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self
            .virtual_file_system
            .close_without_waiting(
                &ItemStatic::Directory(self.directory),
                None,
                &mut self.context,
            )
            .map_err(|e| {
                log::error!("Error closing directory: {}", e);
            });
    }
}

impl Iterator for SynchronousDirectory<'_> {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        self.read().unwrap()
    }
}
