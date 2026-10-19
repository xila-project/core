use crate::{XilaFileSystemFile, abi_unsafe_function};

use virtual_file_system::Error;

abi_unsafe_function! {
    /// This function is used to send data through a socket.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    fn xila_network_send(
        _socket:  XilaFileSystemFile,
        buffer: *const u8,
        size: usize,
    ) -> XilaFileSystemResult {
        if buffer.is_null() {
            return Err(Error::InvalidParameter);
        }

        let _buffer = core::slice::from_raw_parts(buffer, size);

        Ok(())
    }
}

abi_unsafe_function! {
    /// This function is used to receive data through a socket.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it dereferences raw pointers.
    ///
    /// # Errors
    ///
    /// This function may return an error if the file system fails to receive the data.
    fn xila_network_receive(
        _socket: XilaFileSystemFile,
        buffer: *mut u8,
        size: usize,
    ) -> XilaFileSystemResult {
        if buffer.is_null() {
            return Err(Error::InvalidParameter);
        }

        let _buffer = core::slice::from_raw_parts_mut(buffer, size);

        Ok(())
    }
}
