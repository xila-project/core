use file_system::{AccessFlags, CreateFlags, Kind, Position, StateFlags};

use crate::{XilaFileSystemFile, XilaGroupIdentifier, XilaTime, XilaUserIdentifier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum XilaFileSystemWhence {
    Start,
    Current,
    End,
}

pub const fn into_position(whence: XilaFileSystemWhence, offset: i64) -> Position {
    match whence {
        XilaFileSystemWhence::Start => Position::Start(offset as u64),
        XilaFileSystemWhence::Current => Position::Current(offset),
        XilaFileSystemWhence::End => Position::End(offset),
    }
}

#[repr(u8)]
pub enum XilaFileKind {
    File,
    Directory,
    BlockDevice,
    CharacterDevice,
    Pipe,
    Socket,
    SymbolicLink,
}

impl From<XilaFileKind> for file_system::Kind {
    fn from(type_value: XilaFileKind) -> Self {
        match type_value {
            XilaFileKind::File => Kind::File,
            XilaFileKind::Directory => file_system::Kind::Directory,
            XilaFileKind::BlockDevice => file_system::Kind::BlockDevice,
            XilaFileKind::CharacterDevice => file_system::Kind::CharacterDevice,
            XilaFileKind::Pipe => file_system::Kind::Pipe,
            XilaFileKind::Socket => file_system::Kind::Socket,
            XilaFileKind::SymbolicLink => file_system::Kind::SymbolicLink,
        }
    }
}

impl From<file_system::Kind> for XilaFileKind {
    fn from(type_value: file_system::Kind) -> Self {
        match type_value {
            file_system::Kind::File => XilaFileKind::File,
            file_system::Kind::Directory => XilaFileKind::Directory,
            file_system::Kind::BlockDevice => XilaFileKind::BlockDevice,
            file_system::Kind::CharacterDevice => XilaFileKind::CharacterDevice,
            file_system::Kind::Pipe => XilaFileKind::Pipe,
            file_system::Kind::Socket => XilaFileKind::Socket,
            file_system::Kind::SymbolicLink => XilaFileKind::SymbolicLink,
        }
    }
}

pub type XilaFileSystemAccess = u8;

#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_ACCESS_READ_MASK: u8 = AccessFlags::Read.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_ACCESS_WRITE_MASK: u8 = AccessFlags::Write.bits();

pub type XilaFileSystemOpen = u8;

#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_OPEN_CREATE_MASK: u8 = CreateFlags::Create.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_OPEN_CREATE_ONLY_MASK: u8 = CreateFlags::Exclusive.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_OPEN_TRUNCATE_MASK: u8 = CreateFlags::Truncate.bits();
pub type XilaFileSystemState = u8;

#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_STATE_APPEND_MASK: u8 = StateFlags::Append.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_STATE_NON_BLOCKING_MASK: u8 = StateFlags::NonBlocking.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_STATE_SYNCHRONOUS_MASK: u8 = StateFlags::Synchronous.bits();
#[unsafe(no_mangle)]
pub static XILA_FILE_SYSTEM_STATE_SYNCHRONOUS_DATA_ONLY_MASK: u8 =
    StateFlags::SynchronousDataOnly.bits();

pub type XilaFileSystemInode = u64;

type XilaFileSystemIdentifier = u32;

type Permissions = u16;

#[repr(C)]
pub struct XilaFileSystemStatistics {
    file_system: XilaFileSystemIdentifier,
    inode: XilaFileSystemInode,
    links: u64,
    size: XilaFileSystemSize,
    creation: XilaTime,
    access: XilaTime,
    modification: XilaTime,
    status: XilaTime,
    kind: XilaFileKind,
    permissions: Permissions,
    user: XilaUserIdentifier,
    group: XilaGroupIdentifier,
}

impl XilaFileSystemStatistics {
    pub fn from_statistics(statistics: file_system::Statistics) -> Self {
        Self {
            file_system: 0,
            inode: statistics.inode,
            links: statistics.links,
            size: statistics.size,
            creation: statistics.creation.as_u64(),
            access: statistics.access.as_u64(),
            modification: statistics.modification.as_u64(),
            status: statistics.status.as_u64(),
            kind: statistics.kind.into(),
            permissions: statistics.permissions.as_u16(),
            user: statistics.user.as_u16(),
            group: statistics.user.as_u16(),
        }
    }

    /// # Safety
    ///
    /// This function is unsafe because it dereferences a raw pointer.
    /// The caller must ensure that the pointer is valid and properly aligned.
    pub unsafe fn from_mutable_pointer<'a>(
        pointer: *mut XilaFileSystemStatistics,
    ) -> Option<&'a mut XilaFileSystemStatistics> {
        if pointer.is_null() {
            return None;
        }

        if !(pointer as usize).is_multiple_of(align_of::<XilaFileSystemStatistics>()) {
            return None;
        }

        Some(unsafe { &mut *pointer })
    }
}

pub type XilaFileSystemSize = u64;

pub type XilaFileSystemResult = u32;

pub const XILA_RESULT_OK: u32 = 0;

#[repr(C)]
pub struct XilaFileSystemPollEvent {
    fd: XilaFileSystemFile,
    events: u32,
    revents: u32,
}
