//! LittleFS calls these C callbacks synchronously: they cannot suspend on I/O.
//! Probe the device's polling contract once and cancel its registration. Pending
//! or a short transfer maps to LFS_ERR_IO; asynchronous storage needs a separate
//! bridge rather than a busy loop in this callback.

use core::ffi::{c_int, c_void};

use alloc::slice;

use crate::configuration::Context;

use super::littlefs;

pub unsafe extern "C" fn read_callback(
    configuration: *const littlefs::lfs_config,
    block: littlefs::lfs_block_t,
    offset: littlefs::lfs_off_t,
    buffer: *mut c_void,
    size: littlefs::lfs_size_t,
) -> c_int {
    let context = unsafe { Context::get_from_configuration(configuration) };

    let buffer = unsafe { slice::from_raw_parts_mut(buffer as *mut u8, size as usize) };

    let block_size = unsafe { configuration.read().block_size };

    let position = block as u64 * block_size as u64 + offset as u64;

    {
        match context.device.read_sync(buffer, position) {
            Ok(count) if count == buffer.len() => (),

            _ => return littlefs::lfs_error_LFS_ERR_IO,
        }
    }

    0
}

pub unsafe extern "C" fn programm_callback(
    configuration: *const littlefs::lfs_config,
    block: littlefs::lfs_block_t,
    offset: littlefs::lfs_off_t,
    buffer: *const c_void,
    size: littlefs::lfs_size_t,
) -> c_int {
    let context = unsafe { Context::get_from_configuration(configuration) };
    let buffer = unsafe { slice::from_raw_parts(buffer as *const u8, size as usize) };

    let block_size = unsafe { configuration.read().block_size };

    let position = block as u64 * block_size as u64 + offset as u64;

    {
        match context.device.write_sync(buffer, position) {
            Ok(count) if count == buffer.len() => (),

            _ => return littlefs::lfs_error_LFS_ERR_IO,
        }
    }

    0
}

pub unsafe extern "C" fn erase_callback(
    configuration: *const littlefs::lfs_config,
    block: littlefs::lfs_block_t,
) -> c_int {
    let context = unsafe { Context::get_from_configuration(configuration) };
    let block_size = unsafe { configuration.read().block_size };

    let position = block as u64 * block_size as u64;

    {
        match context
            .device
            .write_pattern_sync(&[0x00], block_size as usize, position)
        {
            Ok(count) if count == block_size as usize => (),

            _ => return littlefs::lfs_error_LFS_ERR_IO,
        }
    }

    0
}

pub unsafe extern "C" fn flush_callback(configuration: *const littlefs::lfs_config) -> c_int {
    let context = unsafe { Context::get_from_configuration(configuration) };
    {
        match context.device.flush_sync() {
            Ok(()) => (),

            _ => return littlefs::lfs_error_LFS_ERR_IO,
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context as TaskContext, Poll},
    };
    use file_system::{
        DirectBaseOperations, DirectBlockDevice, MountOperations, Operation, Result, Size,
    };

    struct PendingStorage {
        probes: AtomicUsize,
        cancellations: AtomicUsize,
    }
    impl DirectBaseOperations for PendingStorage {
        fn read(&self, _: &mut TaskContext<'_>, _: &mut [u8], _: Size) -> Poll<Result<usize>> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            Poll::Pending
        }
        fn write(&self, _: &mut TaskContext<'_>, _: &[u8], _: Size) -> Poll<Result<usize>> {
            Poll::Pending
        }
        fn cancel_operation(&self, _: Operation) {
            self.cancellations.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl MountOperations for PendingStorage {}
    impl DirectBlockDevice for PendingStorage {}

    #[test]
    fn synchronous_c_callback_cancels_pending_instead_of_spinning() {
        let device = Box::leak(Box::new(PendingStorage {
            probes: AtomicUsize::new(0),
            cancellations: AtomicUsize::new(0),
        }));
        let mut context = Context { device };
        let configuration = littlefs::lfs_config {
            context: (&mut context as *mut Context).cast(),
            block_size: 512,
            ..crate::configuration::Configuration::DEFAULT_RAW
        };
        let mut bytes = [0; 8];
        assert_eq!(
            unsafe { read_callback(&configuration, 0, 0, bytes.as_mut_ptr().cast(), 8) },
            littlefs::lfs_error_LFS_ERR_IO
        );
        assert_eq!(device.probes.load(Ordering::SeqCst), 1);
        assert_eq!(device.cancellations.load(Ordering::SeqCst), 1);
    }
}
