use crate::{
    Directory, Process, Status, XILA_RIGHT_READ, XILA_RIGHT_WRITE, canonical, path_input,
    registry::{self, Handle},
};
use core::{
    ffi::{c_char, c_void},
    slice,
};
use file_system::{AccessFlags, CreateFlags, Flags, Position, StateFlags};
use virtual_file_system::{File as VfsFile, FileReadiness, SynchronousFile};

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
    // Async I/O can suspend a coroutine. Other callers still probe once.
    pub(crate) inner: VfsFile<'static>,
    pub(crate) readiness: FileReadiness,
}
impl File {
    pub fn new(inner: SynchronousFile<'static>) -> Self {
        let readiness = inner.readiness();
        Self {
            inner: VfsFile::from_synchronous_file(inner),
            readiness,
        }
    }
    /// Hand the file to a program as an owned pointer. Inside a native process the file
    /// is also registered, so the host closes it if the process ends without doing so.
    pub fn into_raw(self) -> *mut Self {
        registry::register(self)
    }
    /// Reclaim an owned file.
    /// # Safety
    /// `raw` must be a live pointer returned by `into_raw`, reclaimed exactly once.
    pub unsafe fn from_raw(raw: *mut Self) -> Self {
        unsafe { registry::reclaim(raw) }
    }
}

impl Handle for File {
    unsafe fn close(raw: *mut Self) {
        // SAFETY: the registry holds only live files that came from `into_raw`.
        let _ = unsafe { xila_file_close(raw) };
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
    // SAFETY: checked non-null above; the caller guarantees the object is live.
    let directory = unsafe { &*directory };
    let result = (|| {
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
pub unsafe extern "C" fn xila_file_read(
    file: *mut File,
    buffer: *mut core::ffi::c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    unsafe { file_read(file, buffer, size, None, count) }
}

/// Transfer at an explicit offset without changing the shared cursor.
/// Streams return InvalidSeek.
/// # Safety
/// Same requirements as `xila_file_read`.
pub unsafe extern "C" fn xila_file_read_at(
    file: *mut File,
    buffer: *mut c_void,
    size: usize,
    offset: u64,
    count: *mut usize,
) -> Status {
    unsafe { file_read(file, buffer, size, Some(offset), count) }
}

unsafe fn file_read(
    file: *mut File,
    buffer: *mut c_void,
    size: usize,
    offset: Option<u64>,
    count: *mut usize,
) -> Status {
    if file.is_null()
        || count.is_null()
        || (size != 0 && buffer.is_null())
        || size > isize::MAX as usize
        || offset.is_some_and(|offset| offset.checked_add(size as u64).is_none())
    {
        return Status::InvalidArgument;
    }
    let buffer = if size == 0 {
        &mut []
    } else {
        unsafe { slice::from_raw_parts_mut(buffer.cast(), size) }
    };
    let file = unsafe { &mut (*file).inner };
    let result = if crate::is_in_coroutine() {
        crate::suspend_on(file.read_with_offset(buffer, offset))
    } else {
        let file = file.as_synchronous_file_mut();
        match offset {
            Some(offset) => file.read_at(buffer, offset),
            None => file.read(buffer),
        }
    };
    match result {
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
pub unsafe extern "C" fn xila_file_write(
    file: *mut File,
    buffer: *const core::ffi::c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    unsafe { file_write(file, buffer, size, None, count) }
}

/// Transfer at an explicit offset without changing the shared cursor.
/// Streams return InvalidSeek.
/// # Safety
/// Same requirements as `xila_file_write`.
pub unsafe extern "C" fn xila_file_write_at(
    file: *mut File,
    buffer: *const c_void,
    size: usize,
    offset: u64,
    count: *mut usize,
) -> Status {
    unsafe { file_write(file, buffer, size, Some(offset), count) }
}

unsafe fn file_write(
    file: *mut File,
    buffer: *const c_void,
    size: usize,
    offset: Option<u64>,
    count: *mut usize,
) -> Status {
    if file.is_null()
        || count.is_null()
        || (size != 0 && buffer.is_null())
        || size > isize::MAX as usize
        || offset.is_some_and(|offset| offset.checked_add(size as u64).is_none())
    {
        return Status::InvalidArgument;
    }
    let buffer = if size == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(buffer.cast(), size) }
    };
    let file = unsafe { &mut (*file).inner };
    let result = if crate::is_in_coroutine() {
        crate::suspend_on(file.write_with_offset(buffer, offset))
    } else {
        let file = file.as_synchronous_file_mut();
        match offset {
            Some(offset) => file.write_at(buffer, offset),
            None => file.write(buffer),
        }
    };
    match result {
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
pub unsafe extern "C" fn xila_file_seek(
    file: *mut File,
    offset: i64,
    whence: u32,
    position: *mut u64,
) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    if unsafe { &(*file).readiness }.is_pipe() {
        return Status::InvalidSeek;
    }
    let seek = match whence {
        0 if offset >= 0 => Position::Start(offset as u64),
        1 => Position::Current(offset),
        2 => Position::End(offset),
        _ => return Status::InvalidArgument,
    };
    match unsafe { &mut *file }
        .inner
        .as_synchronous_file_mut()
        .set_position(&seek)
    {
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
pub unsafe extern "C" fn xila_file_flush(file: *mut File) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    let file = unsafe { &mut (*file).inner };
    let result = if crate::is_in_coroutine() {
        crate::suspend_on(file.flush())
    } else {
        file.as_synchronous_file_mut().flush()
    };
    result.map_or_else(Status::from, |()| Status::Ok)
}
/// Close and free the object even if closing fails. Null succeeds.
/// # Safety
/// Non-null file must be owned and released exactly once, with no concurrent calls.
pub unsafe extern "C" fn xila_file_close(file: *mut File) -> Status {
    if file.is_null() {
        return Status::Ok;
    }
    // Closed in place: the file stays registered while the close waits for the file system,
    // so a process cancelled in the middle leaves it to the host, which closes it again.
    let result = unsafe { &mut (*file).inner }
        .as_synchronous_file_mut()
        .close_in_place();
    drop(unsafe { File::from_raw(file) });
    result.map_or_else(Status::from, |()| Status::Ok)
}

/// Resize a writable file without changing its cursor.
/// # Safety
/// File must be live and exclusively accessed.
pub unsafe extern "C" fn xila_file_set_size(file: *mut File, size: u64) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    unsafe { &mut *file }
        .inner
        .as_synchronous_file_mut()
        .set_size(size)
        .map_or_else(Status::from, |()| Status::Ok)
}

/// Return an independently closable handle with the same cursor and status flags.
/// # Safety
/// File must be live. Serialize with close and operations that mutably borrow
/// the file. Output must be aligned, writable and disjoint from the file.
pub unsafe extern "C" fn xila_file_duplicate(file: *const File, output: *mut *mut File) -> Status {
    if file.is_null() || output.is_null() {
        return Status::InvalidArgument;
    }
    match crate::suspend_on(unsafe { &(*file).inner }.duplicate()) {
        Ok(duplicate) => {
            unsafe { output.write(File::new(duplicate.into_synchronous_file()).into_raw()) };
            Status::Ok
        }
        Err(error) => error.into(),
    }
}

pub const XILA_CONTROL_BYTES_AVAILABLE: u32 = 1;
pub const XILA_CONTROL_GET_FILE_FLAGS: u32 = 2;
pub const XILA_CONTROL_SET_FILE_FLAGS: u32 = 3;

/// A bounded command with one native-endian uint32_t input or output.
/// BYTES_AVAILABLE returns the queued byte count on either pipe end.
/// GET_FILE_FLAGS returns the access mode and status as XILA_OPEN_* bits.
/// SET_FILE_FLAGS replaces the append and nonblocking bits shared by duplicates.
/// # Safety
/// File must be live. Serialize access to this handle with close and any other
/// operations on this handle. A used buffer must be aligned for a uint32_t and
/// disjoint from the file. Input must be readable for input_size bytes and
/// output writable for output_size bytes.
pub unsafe extern "C" fn xila_file_control(
    file: *mut File,
    command: u32,
    input: *const c_void,
    input_size: usize,
    output: *mut c_void,
    output_size: usize,
) -> Status {
    if file.is_null() {
        return Status::InvalidArgument;
    }
    let file = unsafe { &mut (*file).inner }.as_synchronous_file_mut();
    let result = match command {
        XILA_CONTROL_BYTES_AVAILABLE | XILA_CONTROL_GET_FILE_FLAGS => {
            let output = output.cast::<u32>();
            if input_size != 0
                || output_size != size_of::<u32>()
                || output.is_null()
                || !output.is_aligned()
            {
                return Status::InvalidArgument;
            }
            let value = if command == XILA_CONTROL_BYTES_AVAILABLE {
                // All pipes exposed through this ABI have bounded capacities.
                file.pipe_bytes_available()
                    .map(|bytes| u32::try_from(bytes).unwrap_or(u32::MAX))
            } else {
                file_flags(file)
            };
            value.map(|value| unsafe { output.write(value) })
        }
        XILA_CONTROL_SET_FILE_FLAGS => {
            let input = input.cast::<u32>();
            if input_size != size_of::<u32>()
                || output_size != 0
                || input.is_null()
                || !input.is_aligned()
            {
                return Status::InvalidArgument;
            }
            let flags = unsafe { input.read() };
            if flags & !(XILA_OPEN_APPEND | XILA_OPEN_NON_BLOCKING) != 0 {
                return Status::InvalidArgument;
            }
            file.get_state().and_then(|mut state| {
                state.set(StateFlags::Append, flags & XILA_OPEN_APPEND != 0);
                state.set(StateFlags::NonBlocking, flags & XILA_OPEN_NON_BLOCKING != 0);
                file.set_state(state)
            })
        }
        _ => return Status::Unsupported,
    };
    result.map_or_else(Status::from, |()| Status::Ok)
}

fn file_flags(file: &SynchronousFile<'_>) -> file_system::Result<u32> {
    let access = file.get_access()?;
    let state = file.get_state()?;
    Ok(
        (u32::from(access.contains(AccessFlags::Read)) * XILA_OPEN_READ)
            | (u32::from(access.contains(AccessFlags::Write)) * XILA_OPEN_WRITE)
            | (u32::from(state.contains(StateFlags::Append)) * XILA_OPEN_APPEND)
            | (u32::from(state.contains(StateFlags::NonBlocking)) * XILA_OPEN_NON_BLOCKING),
    )
}

/// Report whether this file is an interactive terminal.
/// # Safety
/// File must be live and exclusively accessed. Output must be writable and
/// disjoint from the file.
pub unsafe extern "C" fn xila_file_is_terminal(file: *mut File, terminal: *mut bool) -> Status {
    if file.is_null() || terminal.is_null() {
        return Status::InvalidArgument;
    }
    let file = unsafe { &mut (*file).inner };
    let command = file_system::character_device::IS_A_TERMINAL;
    let result = if crate::is_in_coroutine() {
        crate::suspend_on(file.control(command, &()))
    } else {
        file.as_synchronous_file_mut().control(command, &())
    };
    let value = match result {
        Ok(value) => value,
        Err(file_system::IoErrorKind::Unsupported) => false,
        Err(error) => return error.into(),
    };
    unsafe { terminal.write(value) };
    Status::Ok
}

pub const XILA_PIPE_CAPACITY: usize = 512;
const _: () = assert!(XILA_PIPE_CAPACITY == virtual_file_system::PIPE_BUF);

/// Create an unnamed pipe. Only XILA_OPEN_NON_BLOCKING is accepted in flags.
/// # Safety
/// Process must be a live capability. Both outputs must be aligned, writable,
/// distinct and disjoint from the process.
pub unsafe extern "C" fn xila_pipe_create(
    process: *const Process,
    flags: u32,
    reader: *mut *mut File,
    writer: *mut *mut File,
) -> Status {
    if process.is_null()
        || reader.is_null()
        || writer.is_null()
        || reader == writer
        || flags & !XILA_OPEN_NON_BLOCKING != 0
    {
        return Status::InvalidArgument;
    }
    let mut state = StateFlags::empty();
    state.set(StateFlags::NonBlocking, flags & XILA_OPEN_NON_BLOCKING != 0);
    let vfs = unsafe { (*process).host }.virtual_file_system();
    match crate::suspend_on(vfs.create_unnamed_pipe(XILA_PIPE_CAPACITY, state)) {
        Ok((input, output)) => {
            unsafe {
                reader.write(File::new(input.into_synchronous_file()).into_raw());
                writer.write(File::new(output.into_synchronous_file()).into_raw());
            }
            Status::Ok
        }
        Err(error) => error.into(),
    }
}
