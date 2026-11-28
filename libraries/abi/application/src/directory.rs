use crate::{
    Status,
    registry::{self, Handle},
};
use core::{ffi::c_char, slice, str};
use file_system::Path;
use synchronization::Arc;
use virtual_file_system::{DirectoryCapability, VirtualFileSystem};

pub const XILA_RIGHT_READ: u32 = 1;
pub const XILA_RIGHT_WRITE: u32 = 2;
/// Pass with directory open rights to reject a final symbolic link.
pub const XILA_DIRECTORY_NO_FOLLOW: u32 = 4;

/// Opaque path capability. Native callers must serialize access and preserve ownership.
pub struct Directory {
    pub(crate) vfs: &'static VirtualFileSystem,
    pub(crate) task: task::TaskIdentifier,
    pub(crate) root: Arc<DirectoryCapability<'static>>,
    pub(crate) rights: u8,
}
impl Directory {
    /// Validate an absolute directory and pin it for confined lookups.
    pub fn new(
        vfs: &'static VirtualFileSystem,
        task: task::TaskIdentifier,
        path: &str,
        rights: u32,
    ) -> Result<Self, Status> {
        crate::suspend_on_cancellable(Self::new_async(vfs, task, path, rights))
    }

    pub(crate) async fn new_async(
        vfs: &'static VirtualFileSystem,
        task: task::TaskIdentifier,
        path: &str,
        rights: u32,
    ) -> Result<Self, Status> {
        validate_rights(rights)?;
        if !path.starts_with('/') {
            return Err(Status::InvalidArgument);
        }
        let trimmed = path.trim_end_matches('/');
        let path = if trimmed.is_empty() {
            Path::ROOT
        } else {
            Path::new(trimmed)
        };
        if !path.is_valid() {
            return Err(Status::InvalidArgument);
        }
        let root = Arc::new(vfs.pin_directory(task, path).await?);
        Ok(Self {
            vfs,
            task,
            root,
            rights: rights as u8,
        })
    }

    /// Grant the same directory to a child, which will use its own task authority.
    pub(crate) fn duplicate(&self) -> Self {
        Self {
            vfs: self.vfs,
            task: self.task,
            root: self.root.clone(),
            rights: self.rights,
        }
    }
    /// Hand the capability to a program as an owned pointer. Inside a native process it is
    /// also registered, so the host frees it if the process ends without doing so.
    pub fn into_raw(self) -> *mut Self {
        registry::register(self)
    }
    /// Reclaim an owned capability.
    /// # Safety
    /// `raw` must be a live pointer returned by `into_raw`, reclaimed exactly once.
    pub unsafe fn from_raw(raw: *mut Self) -> Self {
        unsafe { registry::reclaim(raw) }
    }
}

impl Handle for Directory {
    unsafe fn close(raw: *mut Self) {
        // SAFETY: the registry holds only live capabilities that came from `into_raw`.
        let _ = unsafe { xila_directory_close(raw) };
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
/// Validate syntax and leave all components for the confined resolver.
pub(crate) fn relative_path(path: &str, entry: bool) -> Result<&Path, Status> {
    let path = Path::new(path);
    if path.is_absolute()
        || !Path::new(path.as_str().trim_end_matches('/')).is_valid()
        || (entry
            && path
                .as_str()
                .split('/')
                .all(|part| part.is_empty() || part == "."))
    {
        return Err(Status::InvalidArgument);
    }
    Ok(path)
}
#[inline]
pub(crate) unsafe fn path_input<'a>(path: *const c_char, length: usize) -> Result<&'a str, Status> {
    if path.is_null() || length > isize::MAX as usize {
        return Err(Status::InvalidArgument);
    }
    str::from_utf8(unsafe { slice::from_raw_parts(path.cast(), length) })
        .map_err(|_| Status::InvalidArgument)
}

/// Open a relative directory. Requested rights must be a subset of the parent's.
/// Include XILA_DIRECTORY_NO_FOLLOW to reject a final symbolic link.
/// # Safety
/// Non-null objects must be live. Path must be readable for its length and output
/// writable and aligned. Outputs must not overlap inputs. Calls must be serialized.
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
        let follow_final = rights & XILA_DIRECTORY_NO_FOLLOW == 0;
        let rights = rights & !XILA_DIRECTORY_NO_FOLLOW;
        validate_rights(rights)?;
        if rights & !u32::from(parent.rights) != 0 {
            return Err(Status::PermissionDenied);
        }
        // SAFETY: The caller supplies readable path bytes for path_length.
        let path = relative_path(unsafe { path_input(path, path_length)? }, false)?;
        crate::suspend_on_cancellable(async {
            let root = Arc::new(
                parent
                    .root
                    .pin_directory(parent.task, path, follow_final)
                    .await?,
            );
            Ok(Directory {
                vfs: parent.vfs,
                task: parent.task,
                root,
                rights: rights as u8,
            })
        })
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
pub unsafe extern "C" fn xila_directory_close(directory: *mut Directory) -> Status {
    if !directory.is_null() {
        drop(unsafe { Directory::from_raw(directory) });
    }
    Status::Ok
}

impl Directory {
    /// # Safety
    /// Keep non-null path bytes readable for `length` and for the returned borrow.
    /// Calls are serialized, and cancellation drops the waiting future before the process memory is released.
    pub(crate) unsafe fn relative_path<'a>(
        &self,
        path: *const c_char,
        length: usize,
        right: u32,
        nonempty: bool,
    ) -> Result<&'a Path, Status> {
        if path.is_null() {
            return Err(Status::InvalidArgument);
        }
        if u32::from(self.rights) & right == 0 {
            return Err(Status::PermissionDenied);
        }
        // SAFETY: The caller keeps the path bytes readable for the returned borrow.
        relative_path(unsafe { path_input(path, length)? }, nonempty)
    }
}

/// Create a relative directory with the WRITE right.
/// # Safety
/// Directory must be live; path readable for length. Calls must be serialized.
pub unsafe extern "C" fn xila_directory_create(
    directory: *const Directory,
    path: *const c_char,
    length: usize,
) -> Status {
    if directory.is_null() {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let directory = unsafe { &*directory };
    let result = (|| {
        // SAFETY: The caller supplies readable path bytes for length.
        let path = unsafe { directory.relative_path(path, length, XILA_RIGHT_WRITE, true)? };
        crate::suspend_on_cancellable(directory.root.create_directory(directory.task, path))
            .map_err(Status::from)
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Allow removing directories as well as files. Removal never follows the final link.
pub const XILA_REMOVE_DIRECTORY: u32 = 1;

/// Remove a relative entry; XILA_REMOVE_DIRECTORY permits directory removal.
/// # Safety
/// Directory must be live; path readable for length. Calls must be serialized.
pub unsafe extern "C" fn xila_directory_remove(
    directory: *const Directory,
    path: *const c_char,
    length: usize,
    flags: u32,
) -> Status {
    if directory.is_null() || flags & !XILA_REMOVE_DIRECTORY != 0 {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let directory = unsafe { &*directory };
    let result = (|| {
        // SAFETY: The caller supplies readable path bytes for length.
        let path = unsafe { directory.relative_path(path, length, XILA_RIGHT_WRITE, true)? };
        crate::suspend_on_cancellable(directory.root.remove(
            directory.task,
            path,
            flags & XILA_REMOVE_DIRECTORY != 0,
        ))
        .map_err(Status::from)
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Rename an entry using two WRITE capabilities with the same VFS and task.
/// Existing destinations follow the underlying filesystem's replacement semantics.
/// # Safety
/// Objects must be live and paths readable for their lengths. Calls must be serialized.
pub unsafe extern "C" fn xila_directory_rename(
    from: *const Directory,
    from_path: *const c_char,
    from_length: usize,
    to: *const Directory,
    to_path: *const c_char,
    to_length: usize,
) -> Status {
    if from.is_null() || to.is_null() {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees the objects are live.
    let (from, to) = unsafe { (&*from, &*to) };
    let result = (|| {
        // SAFETY: The caller supplies readable source path bytes for from_length.
        let from_path =
            unsafe { from.relative_path(from_path, from_length, XILA_RIGHT_WRITE, true)? };
        // SAFETY: The caller supplies readable destination path bytes for to_length.
        let to_path = unsafe { to.relative_path(to_path, to_length, XILA_RIGHT_WRITE, true)? };
        if !core::ptr::eq(from.vfs, to.vfs) || from.task != to.task {
            return Err(Status::InvalidArgument);
        }
        crate::suspend_on_cancellable(from.root.rename(from.task, from_path, &to.root, to_path))
            .map_err(Status::from)
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}
