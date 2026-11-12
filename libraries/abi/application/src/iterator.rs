use crate::{Directory, Status, XILA_RIGHT_READ};
use alloc::boxed::Box;
use core::ffi::c_char;
use file_system::Entry;
use virtual_file_system::SynchronousDirectory;

/// Opaque owned listing with at most one pending VFS entry for buffer retries.
pub struct DirectoryIterator {
    inner: SynchronousDirectory<'static>,
    pending: Option<Entry>,
}

/// Open a relative directory for listing with READ authority; empty means itself.
/// # Safety
/// Directory must be live; path readable for length. Output must be aligned,
/// writable and disjoint from inputs. Calls must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_iterator_open(
    directory: *const Directory,
    path: *const c_char,
    length: usize,
    output: *mut *mut DirectoryIterator,
) -> Status {
    if directory.is_null() || output.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| {
        let directory = unsafe { &*directory };
        let path = unsafe { directory.resolve(path, length, XILA_RIGHT_READ, false)? };
        let inner = SynchronousDirectory::open(directory.vfs, directory.task, &path)?;
        Ok::<_, Status>(DirectoryIterator {
            inner,
            pending: None,
        })
    })();
    match result {
        Ok(value) => {
            unsafe {
                output.write(Box::into_raw(Box::new(value)));
            }
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Copy a name without a trailing NUL, skipping dot entries. End leaves outputs
/// unchanged. BufferTooSmall writes only required name_length and retains the entry.
/// # Safety
/// Iterator must be live and exclusively accessed. Buffer must be writable for
/// capacity bytes (null allowed only at zero capacity). Outputs must be aligned,
/// writable and disjoint from each other, the buffer and iterator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_iterator_next(
    iterator: *mut DirectoryIterator,
    name_buffer: *mut c_char,
    capacity: usize,
    name_length: *mut usize,
    kind: *mut u32,
) -> Status {
    if iterator.is_null()
        || name_length.is_null()
        || kind.is_null()
        || (capacity != 0 && name_buffer.is_null())
        || capacity > isize::MAX as usize
    {
        return Status::InvalidArgument;
    }
    let iterator = unsafe { &mut *iterator };
    while iterator.pending.is_none() {
        match iterator.inner.read() {
            Ok(Some(entry)) if entry.name == "." || entry.name == ".." => continue,
            Ok(Some(entry)) => iterator.pending = Some(entry),
            Ok(None) => return Status::End,
            Err(error) => return error.into(),
        }
    }
    let entry = iterator.pending.as_ref().unwrap();
    if entry.name.len() > capacity {
        unsafe {
            name_length.write(entry.name.len());
        }
        return Status::BufferTooSmall;
    }
    if !entry.name.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(
                entry.name.as_ptr(),
                name_buffer.cast(),
                entry.name.len(),
            );
        }
    }
    unsafe {
        name_length.write(entry.name.len());
        kind.write(entry.kind as u32);
    }
    iterator.pending = None;
    Status::Ok
}

/// Rewind, discarding a pending entry only on success.
/// # Safety
/// Iterator must be live and exclusively accessed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_iterator_rewind(
    iterator: *mut DirectoryIterator,
) -> Status {
    if iterator.is_null() {
        return Status::InvalidArgument;
    }
    let iterator = unsafe { &mut *iterator };
    match iterator.inner.rewind() {
        Ok(()) => {
            iterator.pending = None;
            Status::Ok
        }
        Err(error) => error.into(),
    }
}

/// Close and free the iterator even on close error; null succeeds.
/// # Safety
/// Non-null iterator must be owned, live and released once; no concurrent calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_directory_iterator_close(iterator: *mut DirectoryIterator) -> Status {
    if iterator.is_null() {
        return Status::Ok;
    }
    let DirectoryIterator { inner, .. } = *unsafe { Box::from_raw(iterator) };
    inner
        .close_owned()
        .map_or_else(Status::from, |()| Status::Ok)
}
