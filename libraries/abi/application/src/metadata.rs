use crate::{Directory, File, Status, XILA_RIGHT_READ};
use core::{ffi::c_char, mem::size_of};

pub const XILA_STATISTICS_NO_FOLLOW: u32 = 1;
pub const XILA_KIND_FILE: u32 = 1;
pub const XILA_KIND_DIRECTORY: u32 = 2;
pub const XILA_KIND_BLOCK_DEVICE: u32 = 3;
pub const XILA_KIND_CHARACTER_DEVICE: u32 = 4;
pub const XILA_KIND_PIPE: u32 = 5;
pub const XILA_KIND_SOCKET: u32 = 6;
pub const XILA_KIND_SYMLINK: u32 = 7;

/// Versioned output: initialize size to sizeof(XilaStatistics) before each call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Statistics {
    pub size: u32,
    pub kind: u32,
    pub permissions: u32,
    pub owner_user: u32,
    pub owner_group: u32,
    pub reserved: u32,
    pub file_size: u64,
    pub link_count: u64,
    pub inode: u64,
    pub access_nanoseconds: i64,
    pub modification_nanoseconds: i64,
    pub status_change_nanoseconds: i64,
}

/// Map the internal kind to its published value; never rely on enum order.
pub(crate) const fn kind_value(kind: file_system::Kind) -> u32 {
    use file_system::Kind;
    match kind {
        Kind::File => XILA_KIND_FILE,
        Kind::Directory => XILA_KIND_DIRECTORY,
        Kind::BlockDevice => XILA_KIND_BLOCK_DEVICE,
        Kind::CharacterDevice => XILA_KIND_CHARACTER_DEVICE,
        Kind::Pipe => XILA_KIND_PIPE,
        Kind::Socket => XILA_KIND_SOCKET,
        Kind::SymbolicLink => XILA_KIND_SYMLINK,
    }
}

impl TryFrom<file_system::Statistics> for Statistics {
    type Error = Status;
    fn try_from(value: file_system::Statistics) -> Result<Self, Status> {
        let nanos = |time: file_system::Time| {
            time.as_u64()
                .checked_mul(1_000_000_000)
                .and_then(|n| i64::try_from(n).ok())
                .ok_or(Status::Overflow)
        };
        Ok(Self {
            size: size_of::<Self>() as u32,
            kind: kind_value(value.kind),
            permissions: u32::from(value.permissions.as_u16()),
            owner_user: u32::from(value.user.as_u16()),
            owner_group: u32::from(value.group.as_u16()),
            reserved: 0,
            file_size: value.size,
            link_count: value.links,
            inode: value.inode,
            access_nanoseconds: nanos(value.access)?,
            modification_nanoseconds: nanos(value.modification)?,
            status_change_nanoseconds: nanos(value.status)?,
        })
    }
}

const STATISTICS_V1_SIZE: u32 = 72;

pub(crate) unsafe fn validate_output(output: *mut Statistics) -> Result<(), Status> {
    if output.is_null()
        || unsafe { core::ptr::addr_of!((*output).size).read() } < STATISTICS_V1_SIZE
    {
        return Err(Status::InvalidArgument);
    }
    Ok(())
}

unsafe fn write_output(output: *mut Statistics, value: &Statistics) {
    let length = (unsafe { core::ptr::addr_of!((*output).size).read() } as usize)
        .min(size_of::<Statistics>());
    // The public layout contains no implicit padding; every copied byte is initialized.
    unsafe {
        core::ptr::copy_nonoverlapping(
            core::ptr::from_ref(value).cast::<u8>(),
            output.cast::<u8>(),
            length,
        );
    }
}

/// Read metadata of an opened readable file.
/// # Safety
/// File must be live and exclusively accessed. Output must be aligned, writable
/// for its declared size, initialized with size, and disjoint from the file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_get_statistics(
    file: *mut File,
    output: *mut Statistics,
) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let file = unsafe { &mut *file };
    let result = (|| {
        unsafe {
            validate_output(output)?;
        }
        // Metadata is not file content: like POSIX fstat, any open file may be queried.
        Statistics::try_from(file.inner.get_statistics()?)
    })();
    match result {
        Ok(value) => {
            unsafe {
                write_output(output, &value);
            }
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Read relative path metadata; an empty path means the capability itself.
/// # Safety
/// Directory must be live; path readable for length. Output must be aligned,
/// writable for its declared size, initialized with size and disjoint from inputs.
/// Calls must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_get_statistics(
    directory: *const Directory,
    path: *const c_char,
    length: usize,
    flags: u32,
    output: *mut Statistics,
) -> Status {
    if directory.is_null() || flags & !XILA_STATISTICS_NO_FOLLOW != 0 {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let directory = unsafe { &*directory };
    let result = (|| {
        unsafe {
            validate_output(output)?;
        }
        let path = unsafe { directory.resolve(path, length, XILA_RIGHT_READ, false)? };
        let value = if flags & XILA_STATISTICS_NO_FOLLOW != 0 {
            task::block_on(
                directory
                    .vfs
                    .get_link_statistics_for_task(directory.task, &path),
            )?
        } else {
            task::block_on(directory.vfs.get_statistics_for_task(directory.task, &path))?
        };
        Statistics::try_from(value)
    })();
    match result {
        Ok(value) => {
            unsafe {
                write_output(output, &value);
            }
            Status::Ok
        }
        Err(error) => error,
    }
}
