use core::{
    ffi::c_char,
    ptr::{self, copy_nonoverlapping},
};
use task::block_on;

use crate::{XilaFileSystemStatistics, XilaTaskIdentifier, abi_unsafe_function, parse_c_str};

abi_unsafe_function! {
    /// This function is used to get statistics for a path.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_get_statistics_from_path(
        path: *const c_char,
        statistics: *mut XilaFileSystemStatistics,
    ) -> XilaFileSystemResult {
        let path = parse_c_str(path)?;

        log::information!("Getting statistics for path {path:?}");

        let result = block_on(crate::runtime_context().virtual_file_system.get_statistics(&path))?;

        log::information!("Got statistics for path {path:?}: {result:?}");

        ptr::write(statistics, XilaFileSystemStatistics::from_statistics(result));
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to convert a path to a resolved path (i.e. a path without symbolic links or relative paths).
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_resolve_path(
        path: *const i8,
        resolved_path: *mut u8,
        resolved_path_size: usize,
    ) -> XilaFileSystemResult {
        // Casting *const i8 to *const c_char so it plays nicely with parse_c_str
        let path = parse_c_str(path as *const c_char)?;

        // Debug: Resolving path

        // Copy path to resolved path.
        copy_nonoverlapping(
            path.as_ptr(),
            resolved_path,
            usize::min(resolved_path_size, path.len()),
        );

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to rename (move) a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_rename(
        task: XilaTaskIdentifier,
        old_path: *const c_char,
        new_path: *const c_char,
    ) -> XilaFileSystemResult {
        let old_path = parse_c_str(old_path)?;
        let new_path = parse_c_str(new_path)?;

        // Debug: Renaming files

        block_on(
            crate::runtime_context()
                .virtual_file_system
                .rename(task.into(), &old_path, &new_path),
        )?;
        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to remove a file.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_file_system_remove(
        task: XilaTaskIdentifier,
        path: *const c_char,
    ) -> XilaFileSystemResult {
        let path = parse_c_str(path)?;

        block_on(crate::runtime_context().virtual_file_system.remove(task.into(), path))
    }
}
