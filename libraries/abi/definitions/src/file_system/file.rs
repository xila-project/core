/// This module implements the POSIX like file system C ABI.
use core::ffi::c_char;
use file_system::{
    AccessFlags, CreateFlags, Flags, Kind, Permissions, StateFlags, Statistics, Time,
    character_device,
};
use shared::generate_shadow_type;
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{Error, SynchronousFile};

use crate::{
    XilaFileSystemPollEvent, XilaFileSystemState, XilaTaskIdentifier, XilaTime,
    abi_unsafe_function, file_system::into_position, parse_c_str,
};

use super::{
    XilaFileSystemAccess, XilaFileSystemOpen, XilaFileSystemSize, XilaFileSystemStatistics,
    XilaFileSystemWhence,
};

generate_shadow_type!(XilaFileSystemFile, SynchronousFile<'static>);

abi_unsafe_function! {
    /// This function is used to get the statistics of a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    ///
    /// # Errors
    /// This function may return an error if the file system fails to get the statistics of the file.
    fn xila_file_system_file_get_statistics(
        file: *mut XilaFileSystemFile,
        statistics: *mut XilaFileSystemStatistics,
    ) -> XilaFileSystemResult {
        log::debug!("Getting statistics for file {file:?}");

        let result = match (*file).get_statistics() {
            Ok(statistics) => statistics,
            // Some character devices don't expose attribute operations.
            // Return minimal synthetic metadata so POSIX callers (e.g. WASI libc)
            // can proceed after open/fstat.
            Err(Error::UnsupportedOperation)
            | Err(Error::FileSystem(file_system::Error::UnsupportedOperation)) => {
                log::warning!(
                    "File system does not support getting statistics for file {file:?}, returning synthetic character device metadata"
                );
                Statistics::new(
                    0,
                    1,
                    0,
                    Time::new(0),
                    Time::new(0),
                    Time::new(0),
                    Time::new(0),
                    Kind::CharacterDevice,
                    Permissions::DEVICE_DEFAULT,
                    UserIdentifier::ROOT,
                    GroupIdentifier::ROOT,
                )
            }
            Err(error) => return Err(error),
        };

        *statistics = XilaFileSystemStatistics::from_statistics(result);
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to get the access mode of a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to get the access mode of the file.
    fn xila_file_system_file_get_access_flags(
        file: *mut XilaFileSystemFile,
        mode: *mut XilaFileSystemAccess,
    ) -> XilaFileSystemResult {
        let m = (*file).get_access()?;
        mode.write(m.bits());
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to close a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to close the file.
    fn xila_file_system_file_close(
        file: *mut XilaFileSystemFile,
    ) -> XilaFileSystemResult {
        log::information!("Closing file {file:?}");
        (*file).close_internal(crate::runtime_context().virtual_file_system)
    }
}

abi_unsafe_function! {
    /// This function is used perform a vectored write operation on a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to open the file.
    fn xila_file_system_file_write(
        file: *mut XilaFileSystemFile,
        buffers: *const *const u8,
        buffers_length: *const usize,
        buffer_count: usize,
        written: *mut usize,
    ) -> XilaFileSystemResult {
        let buffers = core::slice::from_raw_parts(buffers, buffer_count);
        let buffers_length = core::slice::from_raw_parts(buffers_length, buffer_count);

        let mut current_written = 0;

        for (buffer, length) in buffers.iter().zip(buffers_length.iter()) {
            let buffer_slice = core::slice::from_raw_parts(*buffer, *length);
            current_written += (*file).write(buffer_slice)?;
        }

        if !written.is_null() {
            *written = current_written;
        }
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to perform a write operation on a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to open the file.
    fn xila_file_system_file_read(
        file: *mut XilaFileSystemFile,
        buffers: *const *mut u8,
        buffers_length: *const usize,
        buffer_count: usize,
        read: *mut usize,
    ) -> XilaFileSystemResult {
        let buffers = core::slice::from_raw_parts(buffers, buffer_count);
        let buffers_length = core::slice::from_raw_parts(buffers_length, buffer_count);

        let mut current_read = 0;

        for (buffer_pointer, buffer_length) in buffers.iter().zip(buffers_length.iter()) {
            let buffer = core::slice::from_raw_parts_mut(*buffer_pointer, *buffer_length);
            let read_count = (*file).read(buffer)?;
            current_read += read_count;
        }

        if !read.is_null() {
            *read = current_read;
        }
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to perform a read operation on a file at a specific position.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_file_read_at(
        file: *mut XilaFileSystemFile,
        position: u64,
        buffers: *const *mut u8,
        buffers_length: *const usize,
        buffer_count: usize,
        read: *mut usize,
    ) -> XilaFileSystemResult {
        let buffers = core::slice::from_raw_parts(buffers, buffer_count);
        let buffers_length = core::slice::from_raw_parts(buffers_length, buffer_count);

        (*file).set_position(&file_system::Position::Start(position))?;

        let mut current_read = 0;

        for (buffer_pointer, buffer_length) in buffers.iter().zip(buffers_length.iter()) {
            let buffer = core::slice::from_raw_parts_mut(*buffer_pointer, *buffer_length);
            let read_count = (*file).read(buffer)?;
            current_read += read_count;
        }

        if !read.is_null() {
            *read = current_read;
        }
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to perform a write operation on a file at a specific position.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_file_write_at(
        file: *mut XilaFileSystemFile,
        position: u64,
        buffers: *const *const u8,
        buffers_length: *const usize,
        buffer_count: usize,
        written: *mut usize,
    ) -> XilaFileSystemResult {
        let buffers = core::slice::from_raw_parts(buffers, buffer_count);
        let buffers_length = core::slice::from_raw_parts(buffers_length, buffer_count);

        (*file).set_position(&file_system::Position::Start(position))?;

        let mut current_written = 0;

        for (buffer, length) in buffers.iter().zip(buffers_length.iter()) {
            let buffer_slice = core::slice::from_raw_parts(*buffer, *length);
            current_written += (*file).write(buffer_slice)?;
        }

        if !written.is_null() {
            *written = current_written;
        }
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to check if a file is a terminal.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to open the file.
    fn xila_file_system_file_is_a_terminal(
        file: *mut XilaFileSystemFile,
        is_a_terminal: *mut bool,
    ) -> XilaFileSystemResult {
        *is_a_terminal = match (*file).control(character_device::IS_A_TERMINAL, &()) {
            Ok(result) => result,
            Err(Error::UnsupportedOperation) => false,
            Err(_) => false,
        };
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to open a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_file_open(
        task: XilaTaskIdentifier,
        path: *const c_char,
        mode: XilaFileSystemAccess,
        open: XilaFileSystemOpen,
        status: XilaFileSystemState,
        file: *mut XilaFileSystemFile,
    ) -> XilaFileSystemResult {
        log::information!("Opening file at path {:?} with mode {:?}, open {:?} and status {:?}",
            unsafe { parse_c_str(path) },
            mode,
            open,
            status
        );

        let path = parse_c_str(path)?;

        let mode = AccessFlags::from_bits_truncate(mode);
        let open = CreateFlags::from_bits_truncate(open);
        let status = StateFlags::from_bits_truncate(status);

        let flags = Flags::new(mode, Some(open), Some(status));

        log::information!("Resolved flags: {:?}", flags);

        let f = SynchronousFile::open(crate::runtime_context().virtual_file_system, task.into(), path, flags)?;

        log::information!("Successfully opened file at path {:?} with mode {:?}, open {:?} and status {:?}",
            path,
            mode,
            open,
            status
         );

        unsafe {
            let target_slot = file as *mut SynchronousFile<'static>;
            ::core::ptr::write(target_slot, f);
        }

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to set the flags of a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_file_set_flags(
        _file: *mut XilaFileSystemFile,
        _state: XilaFileSystemState,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to get the flags of a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_file_get_state(
        file: *mut XilaFileSystemFile,
        state: *mut XilaFileSystemState,
    ) -> XilaFileSystemResult {
        let s = (*file).get_state()?;
        state.write(s.bits());
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to flush a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFile
    fn xila_file_system_file_flush(
        file: *mut XilaFileSystemFile,
        _t: bool,
    ) -> XilaFileSystemResult {
        (*file).flush()
    }
}

abi_unsafe_function! {
    /// This function is used to set the position in a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_file_set_position(
        file: *mut XilaFileSystemFile,
        offset: i64,
        whence: XilaFileSystemWhence,
        position: *mut XilaFileSystemSize,
    ) -> XilaFileSystemResult {
        let current_position = into_position(whence, offset);

        // Debug: Setting position
        let result = (*file).set_position(&current_position)?;
        *position = result;
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to advice the file system about the access pattern of a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_set_times(
        _file: *mut XilaFileSystemFile,
        _access: XilaTime,
        _modification: XilaTime,
        _flags: u8,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to set access and modification times of a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_set_times_from_path(
        _path: *const c_char,
        _access: XilaTime,
        _modification: XilaTime,
        _flags: u8,
        _follow: bool,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to truncate a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_truncate(
        _file:  *mut XilaFileSystemFile,
        _length: XilaFileSystemSize,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to create a symbolic link.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_link(
        _path: *const c_char,
        _link: *const c_char,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to advice the file system about the access pattern of a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_file_advise(
        _file:  *mut XilaFileSystemFile,
        _offset: XilaFileSystemSize,
        _length: XilaFileSystemSize,
        _advice: u8,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to allocate space for a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_file_allocate(
        _file:  *mut XilaFileSystemFile,
        _offset: XilaFileSystemSize,
        _length: XilaFileSystemSize,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to set the position in a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_file_truncate(
        _file:  *mut XilaFileSystemFile,
        _length: XilaFileSystemSize,
    ) -> XilaFileSystemResult {
        todo!()
    }
}

abi_unsafe_function! {
    /// This function is used to perform a poll operation on a file.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a file.
    fn xila_file_system_dummy(_event: XilaFileSystemPollEvent) -> XilaFileSystemResult {
        Ok(())
    }
}
