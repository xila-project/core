use core::mem::forget;

use exported_file_system::{FileSystemOperations, StateFlags};
use file_system::{Context, Entry, Flags, Path, Size};
use task::TaskIdentifier;
use task::block_on;

use crate::{ItemStatic, Result, SynchronousDirectory, VirtualFileSystem, poll};

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
        virtual_file_system.create_directory(task, &path).await
    }

    pub async fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
    ) -> Result<Self> {
        virtual_file_system.open_directory(task, &path).await
    }

    pub async fn read(&mut self) -> Result<Option<Entry>> {
        poll(|| self.0.read()).await
    }

    pub async fn get_position(&mut self) -> Result<Size> {
        poll(|| self.0.get_position()).await
    }

    pub async fn set_position(&mut self, position: Size) -> Result<()> {
        poll(|| self.0.set_position(position)).await
    }

    pub async fn rewind(&mut self) -> Result<()> {
        poll(|| self.0.set_position(0)).await
    }

    pub async fn close(mut self, virtual_file_system: &VirtualFileSystem) -> Result<()> {
        let result = virtual_file_system
            .close(
                &ItemStatic::Directory(self.0.directory),
                &mut self.0.context,
            )
            .await;
        forget(self.0);

        result
    }

    pub fn into_synchronous_directory(self) -> SynchronousDirectory<'a> {
        self.0
    }
}

impl Iterator for Directory<'_> {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        block_on(self.read()).unwrap()
    }
}
