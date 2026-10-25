use crate::{
    XilaFileSystemAccess, XilaFileSystemStatistics, XilaTaskIdentifier, abi_unsafe_function,
    parse_c_str,
};
use alloc::ffi::CString;
use core::{ffi::c_char, ptr::null_mut};
use log::debug;
use shared::generate_shadow_type;
use task::{TaskIdentifier, block_on};
use virtual_file_system::SynchronousDirectory;

use super::{XilaFileKind, XilaFileSystemInode, XilaFileSystemSize};

generate_shadow_type!(XilaFileSystemDirectory, SynchronousDirectory<'static>);

abi_unsafe_function! {
    /// This function is used to open a directory.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_directory_open(
        task: XilaTaskIdentifier,
        path: *const c_char,
        out_directory: *mut XilaFileSystemDirectory,
    ) -> XilaFileSystemResult {
        let path = parse_c_str(path)?;
        let task = TaskIdentifier::from(task);

        debug!("Opening directory {path:?} for task {task:?}");

        let synchronous_directory =
            SynchronousDirectory::open(crate::runtime_context().virtual_file_system, task, path)?;

        unsafe {
            // Cast the destination slot and overwrite it without reading/dropping garbage first
            let target_slot = out_directory as *mut SynchronousDirectory<'static>;
            ::core::ptr::write(target_slot, synchronous_directory);
        }

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to read a directory.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_directory_read(
        directory: *mut XilaFileSystemDirectory,
        entry_name: *mut *const c_char,
        entry_type: *mut XilaFileKind,
        entry_size: *mut XilaFileSystemSize,
        entry_inode: *mut XilaFileSystemInode,
    ) -> XilaFileSystemResult {
        debug!("Reading directory {directory:?}");

        let entry = (*directory).read()?;

        if let Some(entry) = entry {
            *entry_name = CString::new(entry.name.as_str()).unwrap().into_raw();
            *entry_type = entry.kind.into();
            *entry_size = entry.size;
            *entry_inode = entry.inode;
        } else {
            *entry_name = null_mut();
        }

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to close a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFile
    fn xila_file_system_directory_close(
        directory: *mut XilaFileSystemDirectory,
    ) -> XilaFileSystemResult {
        (*directory).close_internal(crate::runtime_context().virtual_file_system)
    }
}

abi_unsafe_function! {
    /// This function is used to rewind a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a directory.
    fn xila_file_system_directory_rewind(
        directory: *mut XilaFileSystemDirectory,
    ) -> XilaFileSystemResult {
        debug!("Rewinding directory {directory:?} ");

        (*directory).rewind()
    }
}

abi_unsafe_function! {
    /// This function is used to set the position in a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a directory.
    fn xila_file_system_directory_set_position(
        directory: *mut XilaFileSystemDirectory,
        offset: XilaFileSystemSize,
    ) -> XilaFileSystemResult {
        debug!("Setting position in directory {directory:?} to offset {offset}");

        (*directory).set_position(offset)?;

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to get the statistics of a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a directory.
    fn xila_file_system_directory_get_statistics(
        directory: *mut XilaFileSystemDirectory,
        statistics: *mut XilaFileSystemStatistics,
    ) -> XilaFileSystemResult {
        debug!("Getting statistics for directory {directory:?}");

        let result = (*directory).get_statistics()?;

        *statistics = XilaFileSystemStatistics::from_statistics(result);

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to get the access flags of a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a directory.
    fn xila_file_system_directory_get_access(
        directory: *mut XilaFileSystemDirectory,
        access: *mut XilaFileSystemAccess,
    ) -> XilaFileSystemResult {
        debug!("Getting access flags for directory {directory:?}");

        let result = (*directory).get_access()?;

        log::information!("Access flags for directory {directory:?}: {result:?}");

        access.write(result.bits());

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to get the state flags of a directory.
    ///
    /// # Safety
    /// The caller must ensure that the provided pointer is valid and points to a properly initialized `XilaFileSystemItem` that is a directory.
    fn xila_file_system_directory_get_state(
        directory: *mut XilaFileSystemDirectory,
        state: *mut u8,
    ) -> XilaFileSystemResult {
        debug!("Getting state flags for directory {directory:?}");

        let result = (*directory).get_state()?;

        state.write(result.bits());

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to get the position in a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_directory_create(
        task: XilaTaskIdentifier,
        path: *const c_char,
    ) -> XilaFileSystemResult {
        let path = parse_c_str(path)?;

        block_on(crate::runtime_context().virtual_file_system.create_directory(task.into(), &path))?;

        Ok(())
    }
}
