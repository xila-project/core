#![no_std]

extern crate alloc;
use alloc::{boxed::Box, string::String};
use core::{ffi::c_char, slice, str};
use file_system::{AccessFlags, CreateFlags, Flags, PathOwned, Position, StateFlags};
use synchronization::once_lock::OnceLock;
use virtual_file_system::{SynchronousDirectory, SynchronousFile, VirtualFileSystem};

pub const XILA_ABI_VERSION_MAJOR: u32 = 0;
pub const XILA_ABI_VERSION_MINOR: u32 = 1;
pub const XILA_RIGHT_READ: u32 = 1;
pub const XILA_RIGHT_WRITE: u32 = 2;
pub const XILA_OPEN_READ: u32 = 1;
pub const XILA_OPEN_WRITE: u32 = 2;
pub const XILA_OPEN_CREATE: u32 = 4;
pub const XILA_OPEN_EXCLUSIVE: u32 = 8;
pub const XILA_OPEN_TRUNCATE: u32 = 16;
pub const XILA_OPEN_APPEND: u32 = 32;
pub const XILA_OPEN_NON_BLOCKING: u32 = 64;
pub const XILA_SEEK_START: u32 = 0;
pub const XILA_SEEK_CURRENT: u32 = 1;
pub const XILA_SEEK_END: u32 = 2;

/// Stable error values; output arguments are written only on success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Ok = 0,
    InvalidArgument = 1,
    NotFound = 2,
    PermissionDenied = 3,
    AlreadyExists = 4,
    NotADirectory = 5,
    IsADirectory = 6,
    NoSpace = 7,
    OutOfMemory = 8,
    WouldBlock = 9,
    Unsupported = 10,
    Overflow = 11,
    Unavailable = 12,
    InputOutput = 13,
}

impl From<file_system::Error> for Status {
    fn from(error: file_system::Error) -> Self {
        use file_system::Error::*;
        match error {
            PermissionDenied | InvalidMode => Self::PermissionDenied,
            NotFound => Self::NotFound,
            AlreadyExists | DirectoryAlreadyExists => Self::AlreadyExists,
            NotDirectory => Self::NotADirectory,
            IsDirectory => Self::IsADirectory,
            FileSystemFull | NoSpaceLeft => Self::NoSpace,
            NoMemory => Self::OutOfMemory,
            RessourceBusy => Self::WouldBlock,
            UnsupportedOperation => Self::Unsupported,
            FileTooLarge | NameTooLong => Self::Overflow,
            InvalidPath | InvalidParameter | InvalidFlags | InvalidIdentifier | InvalidContext => {
                Self::InvalidArgument
            }
            NotInitialized
            | NotMounted
            | FailedToGetTaskManagerInstance
            | FailedToGetUsersManagerInstance => Self::Unavailable,
            _ => Self::InputOutput,
        }
    }
}
impl From<virtual_file_system::Error> for Status {
    fn from(error: virtual_file_system::Error) -> Self {
        use virtual_file_system::Error::*;
        match error {
            FileSystem(error) => error.into(),
            PermissionDenied | InvalidMode => Self::PermissionDenied,
            AlreadyExists => Self::AlreadyExists,
            NotADirectory => Self::NotADirectory,
            NotAFile => Self::IsADirectory,
            TooManyOpenFiles | TooManyInodes => Self::NoSpace,
            RessourceBusy => Self::WouldBlock,
            UnsupportedOperation => Self::Unsupported,
            InvalidPath | InvalidParameter | InvalidIdentifier | InvalidOpen => {
                Self::InvalidArgument
            }
            UnavailableDriver | Orphaned => Self::Unavailable,
            _ => Self::InputOutput,
        }
    }
}

/// Opaque path capability. Native callers must serialize access and preserve ownership.
pub struct Directory {
    vfs: &'static VirtualFileSystem,
    task: task::TaskIdentifier,
    path: PathOwned,
    rights: u8,
}
/// Opaque owned file. No handle lookup or buffer allocation is required for I/O.
pub struct File {
    inner: SynchronousFile<'static>,
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
impl File {
    pub fn new(inner: SynchronousFile<'static>) -> Self {
        Self { inner }
    }
    pub fn into_raw(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }
    /// Reclaim an owned file.
    /// # Safety
    /// `raw` must be a live pointer returned by `into_raw`, reclaimed exactly once.
    pub unsafe fn from_raw(raw: *mut Self) -> Box<Self> {
        unsafe { Box::from_raw(raw) }
    }
}
#[inline]
fn validate_rights(rights: u32) -> Result<(), Status> {
    if rights & !3 != 0 {
        Err(Status::InvalidArgument)
    } else {
        Ok(())
    }
}
fn canonical(base: &str, path: &str, file: bool) -> Result<PathOwned, Status> {
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
unsafe fn path_input<'a>(path: *const c_char, length: usize) -> Result<&'a str, Status> {
    if path.is_null() || length > isize::MAX as usize {
        return Err(Status::InvalidArgument);
    }
    str::from_utf8(unsafe { slice::from_raw_parts(path.cast(), length) })
        .map_err(|_| Status::InvalidArgument)
}
const XILA_OPEN_ALL: u32 = XILA_OPEN_READ
    | XILA_OPEN_WRITE
    | XILA_OPEN_CREATE
    | XILA_OPEN_EXCLUSIVE
    | XILA_OPEN_TRUNCATE
    | XILA_OPEN_APPEND
    | XILA_OPEN_NON_BLOCKING;
/// Flags that modify file contents and therefore need the WRITE right.
const XILA_OPEN_MODIFYING: u32 =
    XILA_OPEN_WRITE | XILA_OPEN_CREATE | XILA_OPEN_TRUNCATE | XILA_OPEN_APPEND;

fn open_flags(flags: u32, rights: u8) -> Result<Flags, Status> {
    let has = |flag: u32| flags & flag != 0;
    if flags & !XILA_OPEN_ALL != 0
        || !has(XILA_OPEN_READ | XILA_OPEN_WRITE)
        || (has(XILA_OPEN_EXCLUSIVE) && !has(XILA_OPEN_CREATE))
        // Truncating or appending through a read-only file has no defined meaning.
        || (has(XILA_OPEN_TRUNCATE | XILA_OPEN_APPEND) && !has(XILA_OPEN_WRITE))
    {
        return Err(Status::InvalidArgument);
    }
    let rights = u32::from(rights);
    if (has(XILA_OPEN_READ) && rights & XILA_RIGHT_READ == 0)
        || (has(XILA_OPEN_MODIFYING) && rights & XILA_RIGHT_WRITE == 0)
    {
        return Err(Status::PermissionDenied);
    }
    let mut access = AccessFlags::empty();
    access.set(AccessFlags::Read, has(XILA_OPEN_READ));
    access.set(AccessFlags::Write, has(XILA_OPEN_WRITE));
    let mut create = CreateFlags::empty();
    create.set(CreateFlags::Create, has(XILA_OPEN_CREATE));
    create.set(CreateFlags::Exclusive, has(XILA_OPEN_EXCLUSIVE));
    create.set(CreateFlags::Truncate, has(XILA_OPEN_TRUNCATE));
    let mut state = StateFlags::empty();
    state.set(StateFlags::Append, has(XILA_OPEN_APPEND));
    state.set(StateFlags::NonBlocking, has(XILA_OPEN_NON_BLOCKING));
    Ok(Flags::new(access, Some(create), Some(state)))
}

/// Return the ABI version as (major << 16) | minor.
/// # Safety
/// No preconditions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_abi_version() -> u32 {
    (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR
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
    let result = (|| {
        validate_rights(rights)?;
        let parent = unsafe { &*parent };
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

/// Open a file relative to a directory with validated XILA_OPEN flags.
/// # Safety
/// Objects must be live. Path must be readable for its length; output writable,
/// aligned and disjoint from inputs. Calls on an object must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_open(
    directory: *const Directory,
    path: *const c_char,
    path_length: usize,
    flags: u32,
    file: *mut *mut File,
) -> Status {
    if directory.is_null() || file.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| {
        let directory = unsafe { &*directory };
        let flags = open_flags(flags, directory.rights)?;
        let path = canonical(
            directory.path.as_str(),
            unsafe { path_input(path, path_length)? },
            true,
        )?;
        SynchronousFile::open(directory.vfs, directory.task, &path, flags)
            .map(File::new)
            .map_err(Status::from)
    })();
    match result {
        Ok(value) => {
            unsafe { file.write(value.into_raw()) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Read directly into the caller's buffer; zero size permits a null buffer.
/// # Safety
/// File must be live and exclusively accessed. Buffer must be writable for size
/// bytes and disjoint from file and count. Count must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_read(
    file: *mut File,
    buffer: *mut core::ffi::c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    if file.is_null()
        || count.is_null()
        || (size != 0 && buffer.is_null())
        || size > isize::MAX as usize
    {
        return Status::InvalidArgument;
    }
    let buffer = if size == 0 {
        &mut []
    } else {
        unsafe { slice::from_raw_parts_mut(buffer.cast(), size) }
    };
    match unsafe { &mut *file }.inner.read(buffer) {
        Ok(value) => {
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error.into(),
    }
}
/// Write directly from the caller's buffer; zero size permits a null buffer.
/// # Safety
/// File must be live and exclusively accessed. Buffer must be readable for size
/// bytes and disjoint from file and count. Count must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_write(
    file: *mut File,
    buffer: *const core::ffi::c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    if file.is_null()
        || count.is_null()
        || (size != 0 && buffer.is_null())
        || size > isize::MAX as usize
    {
        return Status::InvalidArgument;
    }
    let buffer = if size == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(buffer.cast(), size) }
    };
    match unsafe { &mut *file }.inner.write(buffer) {
        Ok(value) => {
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error.into(),
    }
}
/// Seek using XILA_SEEK_START, CURRENT or END. Position output is optional.
/// # Safety
/// File must be live and exclusively accessed; non-null position must be aligned,
/// writable and disjoint from the object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_seek(
    file: *mut File,
    offset: i64,
    whence: u32,
    position: *mut u64,
) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    let seek = match whence {
        0 if offset >= 0 => Position::Start(offset as u64),
        1 => Position::Current(offset),
        2 => Position::End(offset),
        _ => return Status::InvalidArgument,
    };
    match unsafe { &mut *file }.inner.set_position(&seek) {
        Ok(value) => {
            if !position.is_null() {
                unsafe { position.write(value) };
            }
            Status::Ok
        }
        Err(error) => error.into(),
    }
}
/// Flush pending writes.
/// # Safety
/// File must be live and exclusively accessed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_flush(file: *mut File) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    unsafe { &mut *file }
        .inner
        .flush()
        .map_or_else(Status::from, |()| Status::Ok)
}
/// Close and free the object even if closing fails. Null succeeds.
/// # Safety
/// Non-null file must be owned and released exactly once, with no concurrent calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_file_close(file: *mut File) -> Status {
    if file.is_null() {
        return Status::Ok;
    }
    let File { inner } = *unsafe { File::from_raw(file) };
    inner
        .close_owned()
        .map_or_else(Status::from, |()| Status::Ok)
}

static TIME: OnceLock<&'static time::Manager<'static>> = OnceLock::new();
/// Install the runtime's clock once. Earlier reads return Unavailable.
pub fn initialize(time_manager: &'static time::Manager<'static>) {
    TIME.get_or_init(|| time_manager);
}

/// Read monotonic nanoseconds from an arbitrary epoch; may return Overflow.
/// # Safety
/// Output must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_time_monotonic(nanoseconds: *mut u64) -> Status {
    if nanoseconds.is_null() {
        return Status::InvalidArgument;
    }
    let Some(time) = TIME.try_get() else {
        return Status::Unavailable;
    };
    match u64::try_from(time.monotonic_now().as_nanos()) {
        Ok(value) => {
            unsafe { nanoseconds.write(value) };
            Status::Ok
        }
        Err(_) => Status::Overflow,
    }
}
/// Read signed Unix UTC nanoseconds; unsynchronized clocks return Unavailable.
/// # Safety
/// Output must be aligned and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_time_realtime(nanoseconds: *mut i64) -> Status {
    if nanoseconds.is_null() {
        return Status::InvalidArgument;
    }
    let Some(time) = TIME.try_get() else {
        return Status::Unavailable;
    };
    match time.realtime_now() {
        Ok(value) => match i64::try_from(value.as_nanos()) {
            Ok(value) => {
                unsafe { nanoseconds.write(value) };
                Status::Ok
            }
            Err(_) => Status::Overflow,
        },
        Err(time::Error::Overflow) => Status::Overflow,
        Err(_) => Status::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::ptr;
    testing::instantiate_global_allocator!();
    #[test]
    fn generated_header() {
        let generated = std::fs::read(concat!(env!("OUT_DIR"), "/application.h")).unwrap();
        let committed =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("include/xila/application.h");
        if std::env::var_os("XILA_UPDATE_HEADER").is_some() {
            std::fs::write(&committed, &generated).unwrap();
        }
        assert_eq!(
            std::fs::read(committed).unwrap(),
            generated,
            "regenerate with XILA_UPDATE_HEADER=1"
        );
    }
    #[test]
    fn validation() {
        for path in ["/absolute", "..", "a/../b", "x\0y", ""] {
            assert_eq!(canonical("/root", path, true), Err(Status::InvalidArgument));
        }
        assert_eq!(
            canonical("/root", "./a//./b/", true).unwrap().as_str(),
            "/root/a/b"
        );
        assert_eq!(validate_rights(4), Err(Status::InvalidArgument));
        for flags in [0, 8, 9, 128, u32::MAX, 1 | 16, 1 | 32] {
            assert_eq!(open_flags(flags, 3), Err(Status::InvalidArgument));
        }
        assert_eq!(open_flags(1, 2), Err(Status::PermissionDenied));
        for flags in [2, 5, 2 | 16, 2 | 32] {
            assert_eq!(open_flags(flags, 1), Err(Status::PermissionDenied));
        }
        let flags = open_flags(127, 3).unwrap();
        assert_eq!(flags.get_access(), AccessFlags::READ_WRITE);
        assert_eq!(flags.get_create(), CreateFlags::all());
        assert_eq!(
            flags.get_state(),
            StateFlags::Append | StateFlags::NonBlocking
        );
        let flags = open_flags(XILA_OPEN_READ, 1).unwrap();
        assert_eq!(flags.get_access(), AccessFlags::Read);
        assert_eq!(flags.get_create(), CreateFlags::empty());
        assert_eq!(flags.get_state(), StateFlags::empty());
        unsafe {
            assert_eq!(
                path_input([255u8].as_ptr().cast(), 1),
                Err(Status::InvalidArgument)
            );
        }
    }
    #[test]
    fn mapping() {
        use file_system::Error as E;
        for (error, expected) in [
            (E::NotFound, Status::NotFound),
            (E::NoMemory, Status::OutOfMemory),
            (E::NoSpaceLeft, Status::NoSpace),
            (E::IsDirectory, Status::IsADirectory),
            (E::NotDirectory, Status::NotADirectory),
            (E::RessourceBusy, Status::WouldBlock),
            (E::InvalidMode, Status::PermissionDenied),
            (E::UnsupportedOperation, Status::Unsupported),
            (E::InputOutput, Status::InputOutput),
        ] {
            assert_eq!(Status::from(error), expected);
            assert_eq!(
                Status::from(virtual_file_system::Error::FileSystem(error)),
                expected
            );
        }
    }
    #[test]
    fn nulls_and_uninitialized_time() {
        unsafe {
            assert_eq!(
                xila_file_open(ptr::null(), ptr::null(), 0, 1, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_directory_open(ptr::null(), ptr::null(), 0, 1, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_read(ptr::null_mut(), ptr::null_mut(), 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_write(ptr::null_mut(), ptr::null(), 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_seek(ptr::null_mut(), 0, 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_file_flush(ptr::null_mut()), Status::InvalidArgument);
            assert_eq!(xila_file_close(ptr::null_mut()), Status::Ok);
            xila_directory_close(ptr::null_mut());
            let mut mono = 123;
            let mut real = 456;
            assert_eq!(xila_time_monotonic(&mut mono), Status::Unavailable);
            assert_eq!(xila_time_realtime(&mut real), Status::Unavailable);
            assert_eq!((mono, real), (123, 456));
            assert_eq!(
                xila_time_monotonic(ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_time_realtime(ptr::null_mut()), Status::InvalidArgument);
        }
        std::println!(
            "File={} Directory={}",
            core::mem::size_of::<File>(),
            core::mem::size_of::<Directory>()
        );
    }
}
