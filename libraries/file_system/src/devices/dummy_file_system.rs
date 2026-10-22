use crate::{
    AttributeOperations, Attributes, BaseOperations, Context, DirectoryOperations, Entry,
    FileOperations, FileSystemOperations, Flags, MountOperations, Path, Result, Size,
};

pub struct DummyFileSystem;

impl BaseOperations for DummyFileSystem {
    fn read(
        &self,
        _context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        _buffer: &mut [u8],
        _absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        crate::ready(|| todo!())
    }

    fn write(
        &self,
        _context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        _buffer: &[u8],
        _absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        crate::ready(|| todo!())
    }

    fn clone_context(&self, _context: &Context) -> Result<Context> {
        todo!()
    }
}

impl AttributeOperations for DummyFileSystem {
    fn get_attributes(&self, _context: &mut Context, _attributes: &mut Attributes) -> Result<()> {
        todo!()
    }

    fn set_attributes(&self, _context: &mut Context, _attributes: &Attributes) -> Result<()> {
        todo!()
    }
}

impl FileOperations for DummyFileSystem {}

impl MountOperations for DummyFileSystem {}

impl DirectoryOperations for DummyFileSystem {
    fn read(&self, _context: &mut Context) -> Result<Option<Entry>> {
        todo!()
    }

    fn set_position(&self, _context: &mut Context, _position: Size) -> Result<()> {
        todo!()
    }

    fn get_position(&self, _context: &mut Context) -> Result<Size> {
        todo!()
    }

    fn rewind(&self, _context: &mut Context) -> Result<()> {
        todo!()
    }

    fn close(&self, _context: &mut Context) -> Result<()> {
        todo!()
    }
}

impl FileSystemOperations for DummyFileSystem {
    fn lookup_directory(&self, _context: &mut Context, _path: &Path) -> Result<()> {
        todo!()
    }

    fn lookup_file(&self, _context: &mut Context, _path: &Path, _flags: Flags) -> Result<()> {
        todo!()
    }

    fn create_directory(&self, _path: &Path) -> Result<()> {
        todo!()
    }

    fn create_file(&self, _path: &Path) -> Result<()> {
        todo!()
    }

    fn remove(&self, _path: &Path) -> Result<()> {
        todo!()
    }

    fn rename(&self, _source: &Path, _destination: &Path) -> Result<()> {
        todo!()
    }

    fn get_attributes(&self, _path: &Path, _attributes: &mut Attributes) -> Result<()> {
        todo!()
    }

    fn set_attributes(&self, _path: &Path, _attributes: &Attributes) -> Result<()> {
        todo!()
    }
}
