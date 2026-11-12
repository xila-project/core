use crate::Status;
use alloc::{boxed::Box, string::String};
use core::{ffi::c_char, slice, str};
use file_system::PathOwned;
use virtual_file_system::{SynchronousDirectory, VirtualFileSystem};

pub const XILA_RIGHT_READ: u32 = 1;
pub const XILA_RIGHT_WRITE: u32 = 2;

/// Opaque path capability. Native callers must serialize access and preserve ownership.
pub struct Directory {
    pub(crate) vfs: &'static VirtualFileSystem,
    pub(crate) task: task::TaskIdentifier,
    pub(crate) path: PathOwned,
    pub(crate) rights: u8,
}
impl Directory {
    /// Validate an absolute directory and retain its canonical path and authority.
    pub fn new(
        vfs: &'static VirtualFileSystem,
        task: task::TaskIdentifier,
        path: &str,
        rights: u32,
    ) -> Result<Self, Status> {
        validate_rights(rights)?;
        if !path.starts_with('/') {
            return Err(Status::InvalidArgument);
        }
        let path = canonical("/", &path[1..], false)?;
        Self::validated(vfs, task, path, rights as u8)
    }
    fn validated(
        vfs: &'static VirtualFileSystem,
        task: task::TaskIdentifier,
        path: PathOwned,
        rights: u8,
    ) -> Result<Self, Status> {
        SynchronousDirectory::open(vfs, task, &path)
            .map_err(Status::from)?
            .close(vfs)
            .map_err(Status::from)?;
        Ok(Self {
            vfs,
            task,
            path,
            rights,
        })
    }
    pub fn into_raw(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }
    /// Reclaim an owned capability.
    /// # Safety
    /// `raw` must be a live pointer returned by `into_raw`, reclaimed exactly once.
    pub unsafe fn from_raw(raw: *mut Self) -> Box<Self> {
        unsafe { Box::from_raw(raw) }
    }
}
#[inline]
pub(crate) fn validate_rights(rights: u32) -> Result<(), Status> {
    if rights & !3 != 0 {
        Err(Status::InvalidArgument)
    } else {
        Ok(())
    }
}
pub(crate) fn canonical(base: &str, path: &str, file: bool) -> Result<PathOwned, Status> {
    if path.starts_with('/')
        || path.contains('\0')
        || (file && path.is_empty())
        || path.split('/').any(|part| part == "..")
    {
        return Err(Status::InvalidArgument);
    }
    let mut joined = String::with_capacity(base.len() + path.len() + 1);
    joined.push_str(base);
    for part in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if !joined.ends_with('/') {
            joined.push('/');
        }
        joined.push_str(part);
    }
    PathOwned::new(joined).ok_or(Status::InvalidArgument)
}
#[inline]
pub(crate) unsafe fn path_input<'a>(path: *const c_char, length: usize) -> Result<&'a str, Status> {
    if path.is_null() || length > isize::MAX as usize {
        return Err(Status::InvalidArgument);
    }
    str::from_utf8(unsafe { slice::from_raw_parts(path.cast(), length) })
        .map_err(|_| Status::InvalidArgument)
}

/// Open a relative directory; requested rights must be a subset of the parent's.
/// # Safety
/// Non-null objects must be live. Path must be readable for its length and output
/// writable and aligned. Outputs must not overlap inputs. Calls must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_open(
    parent: *const Directory,
    path: *const c_char,
    path_length: usize,
    rights: u32,
    directory: *mut *mut Directory,
) -> Status {
    if parent.is_null() || directory.is_null() {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let parent = unsafe { &*parent };
    let result = (|| {
        validate_rights(rights)?;
        if rights & !(parent.rights as u32) != 0 {
            return Err(Status::PermissionDenied);
        }
        let path = canonical(
            parent.path.as_str(),
            unsafe { path_input(path, path_length)? },
            false,
        )?;
        Directory::validated(parent.vfs, parent.task, path, rights as u8)
    })();
    match result {
        Ok(value) => {
            unsafe { directory.write(value.into_raw()) };
            Status::Ok
        }
        Err(error) => error,
    }
}
/// Release a directory capability. Null is a no-op.
/// # Safety
/// Non-null pointer must be owned, live, and released exactly once; no concurrent calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_close(directory: *mut Directory) {
    if !directory.is_null() {
        drop(unsafe { Directory::from_raw(directory) });
    }
}
