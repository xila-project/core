use core::fmt::Debug;

use exported_file_system::{
    AttributeOperations, BaseOperations, BlockDevice, CharacterDevice, FileSystemOperations,
    MountOperations,
};

use crate::pipe::Pipe;
use synchronization::Arc;

#[derive(Clone)]
pub enum ItemStatic {
    FileSystem(&'static dyn FileSystemOperations),
    File(&'static dyn FileSystemOperations),
    Directory(&'static dyn FileSystemOperations),
    BlockDevice(&'static dyn BlockDevice),
    CharacterDevice(&'static dyn CharacterDevice),
    Pipe(Arc<Pipe>),
}

impl Debug for ItemStatic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ItemStatic::FileSystem(_) => write!(f, "ItemStatic::FileSystem"),
            ItemStatic::File(_) => write!(f, "ItemStatic::File"),
            ItemStatic::Directory(_) => write!(f, "ItemStatic::Directory"),
            ItemStatic::BlockDevice(_) => write!(f, "ItemStatic::BlockDevice"),
            ItemStatic::CharacterDevice(_) => write!(f, "ItemStatic::CharacterDevice"),
            ItemStatic::Pipe(_) => write!(f, "ItemStatic::Pipe"),
        }
    }
}

impl ItemStatic {
    pub(crate) fn as_base_operations(&self) -> Option<&dyn BaseOperations> {
        match self {
            ItemStatic::File(file_system) => Some(*file_system),
            ItemStatic::BlockDevice(device) => Some(*device),
            ItemStatic::CharacterDevice(device) => Some(*device),
            ItemStatic::Pipe(pipe) => Some(&**pipe),
            _ => None,
        }
    }

    pub(crate) fn as_attributes_operations(&self) -> Option<&dyn AttributeOperations> {
        match self {
            ItemStatic::File(fs) => Some(*fs),
            ItemStatic::Directory(fs) => Some(*fs),
            _ => None,
        }
    }

    pub(crate) fn as_mount_operations(&self) -> Option<&dyn MountOperations> {
        match self {
            ItemStatic::FileSystem(fs) => Some(*fs),
            ItemStatic::BlockDevice(device) => Some(*device),
            ItemStatic::CharacterDevice(device) => Some(*device),
            ItemStatic::Pipe(pipe) => Some(&**pipe),
            _ => None,
        }
    }

    pub(crate) fn as_directory_operations(&self) -> Option<&dyn file_system::DirectoryOperations> {
        match self {
            ItemStatic::Directory(fs) => Some(*fs),
            _ => None,
        }
    }
}
