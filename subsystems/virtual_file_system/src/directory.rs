use core::mem::forget;

use exported_file_system::{FileSystemOperations, StateFlags};
use file_system::{Context, Entry, Flags, Path, Size};
use task::TaskIdentifier;

use crate::{ItemStatic, Result, SynchronousDirectory, VirtualFileSystem, suspend_on};

pub struct Directory<'a>(SynchronousDirectory<'a>);

impl<'a> Directory<'a> {
    pub(crate) fn new(
        virtual_file_system: &'a VirtualFileSystem,
        directory: &'static dyn FileSystemOperations,
        flags: Flags,
        context: Context,
    ) -> Self {
        let flags = Flags::new(
            flags.get_access(),
            Some(flags.get_create()),
            Some(flags.get_state().union(StateFlags::NonBlocking)),
        );

        Self(SynchronousDirectory::new(
            virtual_file_system,
            directory,
            flags,
            context,
        ))
    }

    pub async fn create(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        virtual_file_system
            .create_directory(task, &path.as_ref())
            .await
    }

    pub async fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<Self> {
        virtual_file_system
            .open_directory(task, &path.as_ref())
            .await
    }

    pub async fn read(&mut self) -> Result<Option<Entry>> {
        self.0.read()
    }

    pub async fn get_position(&mut self) -> Result<Size> {
        self.0.get_position()
    }

    pub async fn set_position(&mut self, position: Size) -> Result<()> {
        self.0.set_position(position)
    }

    pub async fn rewind(&mut self) -> Result<()> {
        self.0.set_position(0)
    }

    pub async fn close(mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        let result = virtual_file_system
            .close(
                &ItemStatic::Directory(self.0.directory),
                None,
                &mut self.0.context,
            )
            .await;
        forget(self.0);

        result
    }

    pub(crate) fn into_blocking_directory(mut self) -> SynchronousDirectory<'a> {
        self.0.flags = self.0.flags.set_status(StateFlags::empty());
        self.0
    }

    pub fn into_synchronous_directory(self) -> SynchronousDirectory<'a> {
        self.0
    }
}

impl Iterator for Directory<'_> {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        suspend_on(self.read()).unwrap()
    }
}
