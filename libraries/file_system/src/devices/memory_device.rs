//! In-memory device implementation for testing and simulation.

//!
//! This module provides a memory-based device implementation that stores data
//! in RAM instead of on physical storage. It's primarily used for testing,
//! simulation, and development purposes where you need a device that behaves
//! like storage but doesn't require actual hardware.

use core::fmt::Debug;

use alloc::vec::Vec;
use alloc::{boxed::Box, vec};
use shared::AnyByLayout;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

use crate::block_device::GET_BLOCK_SIZE;
use crate::{
    ControlCommand, DirectBaseOperations, DirectBlockDevice, Error, MountOperations, Result, Size,
    block_device,
};

/// In-memory device implementation with configurable block size.
///
/// This device stores all data in memory using a `Vec<u8>` and provides the same
/// interface as physical storage devices. It's thread-safe and supports all standard
/// device operations. The block size is configurable at compile time through the
/// const generic parameter.
///
/// # Type Parameters
///
/// * `Block_size` - The block size in bytes (must be a power of 2, typically 512)
///
/// # Examples
///
/// ```rust
/// extern crate alloc;
/// use file_system::{MemoryDevice, DirectBaseOperations, Position};
///
/// // Create a 1MB memory device with 512-byte blocks
/// let device = MemoryDevice::<512>::new(1024 * 1024);
///
/// // Write some data
/// let data = b"Hello, Memory Device!";
/// device.write_sync(data, 0).unwrap();
///
/// // Reset position and read back
/// device.set_position(0, &Position::Start(0)).unwrap();
/// let mut buffer = alloc::vec![0u8; data.len()];
/// device.read_sync(&mut buffer, 0).unwrap();
/// assert_eq!(&buffer, data);
/// ```
///
/// # Thread Safety
///
/// The device uses an `RwLock` to ensure thread-safe access to the underlying data.
/// Multiple readers can access the device simultaneously, but writes are exclusive.
pub struct MemoryDevice<const BLOCK_SIZE: u32>(RwLock<CriticalSectionRawMutex, Vec<u8>>);

impl<const BLOCK_SIZE: u32> Debug for MemoryDevice<BLOCK_SIZE> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryDevice")
            .field("size", &self.0.try_read().map(|data| data.len()))
            .finish()
    }
}

impl<const BLOCK_SIZE: u32> MemoryDevice<BLOCK_SIZE> {
    /// Create a new memory device with the specified size.
    ///
    /// The device will be initialized with zeros and have the specified total size.
    /// The size must be a multiple of the block size.
    ///
    /// # Arguments
    ///
    /// * `Size` - Total size of the device in bytes
    ///
    /// # Panics
    ///
    /// Panics if `Size` is not a multiple of `Block_size`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// # use file_system::MemoryDevice;
    ///
    /// // Create a 4KB device with 512-byte blocks
    /// let device = MemoryDevice::<512>::new(4096);
    /// ```
    pub fn new(size: usize) -> Self {
        assert!(size.is_multiple_of(BLOCK_SIZE as usize));

        let data: Vec<u8> = vec![0; size];

        Self(RwLock::new(data))
    }

    pub fn new_static(size: usize) -> &'static Self {
        Box::leak(Box::new(Self::new(size)))
    }

    /// Create a memory device from existing data.
    ///
    /// This allows you to create a device with pre-populated data, useful for
    /// testing with known data patterns or loading device images.
    ///
    /// # Arguments
    ///
    /// * `Data` - Vector containing the initial device data
    ///
    /// # Panics
    ///
    /// Panics if the data length is not a multiple of `Block_size`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// # use file_system::MemoryDevice;
    /// # use alloc::vec;
    ///
    /// // Create device with specific data
    /// let data = vec![0x42; 1024]; // 1KB of 0x42 bytes
    /// let device = MemoryDevice::<512>::from_vec(data);
    /// ```
    pub fn from_vec(data: Vec<u8>) -> Self {
        assert!(data.len().is_multiple_of(BLOCK_SIZE as usize));

        Self(RwLock::new(data))
    }
}

impl<const BLOCK_SIZE: u32> DirectBaseOperations for MemoryDevice<BLOCK_SIZE> {
    /// Read data from the memory device.
    ///
    /// Reads data from the current position into the provided buffer.
    /// The position is automatically advanced by the number of bytes read.
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        crate::ready(|| {
            let inner = self
                .0
                .try_write()
                .map_err(|_| crate::Error::RessourceBusy)?;

            let absolute_position = absolute_position as usize;

            let read_size = buffer
                .len()
                .min(inner.len().saturating_sub(absolute_position));
            buffer[..read_size]
                .copy_from_slice(&inner[absolute_position..absolute_position + read_size]);
            Ok(read_size as _)
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        crate::ready(|| {
            let mut inner = self
                .0
                .try_write()
                .map_err(|_| crate::Error::RessourceBusy)?;

            let absolute_position = absolute_position as usize;

            let write_size = buffer
                .len()
                .min(inner.len().saturating_sub(absolute_position));
            inner[absolute_position..absolute_position + write_size]
                .copy_from_slice(&buffer[..write_size]);

            Ok(write_size as _)
        })
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: crate::ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        crate::ready(|| {
            match command {
                GET_BLOCK_SIZE::IDENTIFIER => {
                    let output = GET_BLOCK_SIZE::cast_output(output)?;
                    *output = BLOCK_SIZE;
                }
                block_device::GET_BLOCK_COUNT::IDENTIFIER => {
                    let output = block_device::GET_BLOCK_COUNT::cast_output(output)?;

                    *output = (self
                        .0
                        .try_read()
                        .map_err(|_| crate::Error::RessourceBusy)?
                        .len()
                        / BLOCK_SIZE as usize) as u32;
                }
                _ => return Err(Error::UnsupportedOperation),
            }

            Ok(())
        })
    }
}

impl<const BLOCK_SIZE: u32> MountOperations for MemoryDevice<BLOCK_SIZE> {}

impl<const BLOCK_SIZE: u32> DirectBlockDevice for MemoryDevice<BLOCK_SIZE> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implement_block_device_tests;

    implement_block_device_tests!(MemoryDevice::<512>::new(4096));
}
