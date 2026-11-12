use crate::{Directory, Status, XILA_RIGHT_READ, XILA_RIGHT_WRITE, canonical, path_input};
use alloc::boxed::Box;
use core::{ffi::c_char, slice};
use file_system::{AccessFlags, CreateFlags, Flags, Position, StateFlags};
use virtual_file_system::SynchronousFile;

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

/// Opaque owned file. No handle lookup or buffer allocation is required for I/O.
pub struct File {
    inner: SynchronousFile<'static>,
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

pub(crate) fn open_flags(flags: u32, rights: u8) -> Result<Flags, Status> {
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
