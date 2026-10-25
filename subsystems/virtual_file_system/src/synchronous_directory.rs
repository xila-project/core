use core::mem::forget;
use exported_file_system::{
    AccessFlags, AttributeFlags, AttributeOperations, Attributes, StateFlags, Statistics,
};
use file_system::{Context, DirectoryOperations, Entry, Flags, Path, Size};
use task::TaskIdentifier;
use task::block_on;

use crate::{Error, ItemStatic, Result, VirtualFileSystem, blocking_operation};

pub struct SynchronousDirectory<'a> {
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
        block_on(virtual_file_system.create_directory(task, &path))
    }

    pub fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<Self> {
        let mut directory =
            block_on(virtual_file_system.open_directory(task, &path))?.into_synchronous_directory();

        directory.flags = directory.flags.set_status(StateFlags::empty());

        Ok(directory)
    }

    pub fn read(&mut self) -> Result<Option<Entry>> {
        let entry = self.blocking_operation(|directory| {
            Ok(DirectoryOperations::read(
                directory.directory,
                &mut directory.context,
            )?)
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

            Statistics::from_attributes(&attributes).ok_or(Error::MissingAttribute)
        })
    }

    pub fn get_position(&mut self) -> Result<Size> {
        self.blocking_operation(|directory| {
            Ok(DirectoryOperations::get_position(
                directory.directory,
                &mut directory.context,
            )?)
        })
    }

    pub fn set_position(&mut self, position: Size) -> Result<()> {
        self.blocking_operation(|directory| {
            Ok(DirectoryOperations::set_position(
                directory.directory,
                &mut directory.context,
                position,
            )?)
        })?;

        Ok(())
    }

    pub fn rewind(&mut self) -> Result<()> {
        self.blocking_operation(|directory| {
            Ok(DirectoryOperations::rewind(
                directory.directory,
                &mut directory.context,
            )?)
        })
    }

    pub fn get_access(&self) -> Result<AccessFlags> {
        Ok(self.flags.get_access())
    }

    pub fn get_state(&self) -> Result<StateFlags> {
        Ok(self.flags.get_state())
    }

    pub fn close_internal(&mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        block_on(
            virtual_file_system.close(&ItemStatic::Directory(self.directory), &mut self.context),
        )
    }

    pub fn close(mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        let result = self.close_internal(virtual_file_system);
        forget(self);

        result
    }
}

impl Drop for SynchronousDirectory<'_> {
    fn drop(&mut self) {
        let _ = self.close_internal(self.virtual_file_system).map_err(|e| {
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
