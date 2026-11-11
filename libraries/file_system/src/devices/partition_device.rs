//! Partition device implementation for accessing individual partitions.

//!
//! This module provides [`PartitionDevice`], which allows treating individual
//! partitions on a storage device as separate devices. This is essential for file
//! systems that need to operate on specific partitions rather than entire disks.

use core::fmt;

use shared::AnyByLayout;

use crate::{
    BaseOperations, ControlCommand, ControlCommandIdentifier, DirectBaseOperations,
    DirectBlockDevice, Error, MountOperations, Position, Result, Size,
    block_device::{self, ERASE, EraseRange, GET_BLOCK_COUNT, GET_BLOCK_SIZE},
};

/// A device implementation that represents a partition within a larger storage device.
///
/// This type wraps a base device and provides access to only a specific region (partition)
/// of that device. It maintains its own position cursor and ensures all operations stay
/// within the partition boundaries. This allows file systems to operate on individual
/// partitions without needing to know about the partition layout.
///
///
/// Note: The partition device does not manage position state internally and does not use atomic operations.
/// Thread safety depends on the underlying base device implementation.
///
///
/// # Examples
///
/// ```rust
/// # extern crate alloc;
/// # use file_system::{MemoryDevice, PartitionDevice, DirectBaseOperations};
///
/// // Create a memory device for testing
/// let base_device = MemoryDevice::<512>::new(1024 * 1024);
///
/// // Create a partition device for blocks 100-199 (100 blocks of 512 bytes = 51.2KB)
/// let partition_device = PartitionDevice::new(&base_device, 100, 100, 512);
///
/// // Now you can use partition_device like any other device
/// let data = b"Hello, Partition!";
/// partition_device.write_sync(data, 0).unwrap();
/// ```
pub struct PartitionDevice<'a, D> {
    /// Base device containing this partition
    base_device: &'a D,
    /// Block size
    block_size: u32,
    /// Byte offset from the beginning of the base device
    offset: Size,
    /// Size of this partition in bytes
    size: Size,
}

impl<'a, D: BaseOperations> PartitionDevice<'a, D> {
    /// Create a new partition device with explicit byte offset and size.
    ///
    /// # Arguments
    ///
    /// * `base_device` - The underlying storage device
    /// * `start_block` - Block index where the partition starts
    /// * `block_count` - Number of blocks in the partition
    /// * `block_size` - Size of each block in bytes
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// # use file_system::{MemoryDevice, PartitionDevice};
    ///
    /// let base_device = MemoryDevice::<512>::new(1024 * 1024);
    /// // Create a partition starting at block 128 (64KB) with 256 blocks (128KB)
    /// let partition = PartitionDevice::new(&base_device, 128, 256, 512);
    /// ```
    pub fn new(base_device: &'a D, start_block: Size, block_count: Size, block_size: u32) -> Self {
        Self {
            base_device,
            block_size,
            offset: start_block * (block_size as Size),
            size: block_count * (block_size as Size),
        }
    }
    /// Get the number of blocks in this partition.
    ///
    /// # Returns
    ///
    /// The number of blocks in this partition, where each block is of the partition's configured block size.
    /// The number of 512-byte sectors this partition contains.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// # use file_system::{MemoryDevice, PartitionDevice};
    ///
    /// let base_device = MemoryDevice::<512>::new(1024 * 1024);
    /// let partition = PartitionDevice::new(&base_device, 100, 50, 512);
    /// assert_eq!(partition.get_block_count(), 50);
    /// ```
    pub const fn get_block_count(&self) -> u32 {
        self.size as u32 / self.block_size
    }

    pub const fn get_start_lba(&self) -> Size {
        self.offset / (self.block_size as Size)
    }

    /// Get the base device
    pub const fn get_base_device(&self) -> &D {
        self.base_device
    }

    /// Check if the partition device is valid (non-zero size)
    pub const fn is_valid(&self) -> bool {
        self.size > 0
    }

    const fn get_device_position(&self, absolute_position: Size) -> Option<Size> {
        if absolute_position >= self.size {
            return None;
        }

        let device_position = match self.offset.checked_add(absolute_position) {
            Some(pos) => pos,
            None => return None,
        };

        Some(device_position)
    }

    const fn get_total_buffer_length(
        &self,
        absolute_position: Size,
        buffer_length: usize,
    ) -> usize {
        let remaining_size = self.size.saturating_sub(absolute_position) as usize;

        if buffer_length > remaining_size {
            remaining_size
        } else {
            buffer_length
        }
    }
}

impl<'a, D: DirectBaseOperations> DirectBaseOperations for PartitionDevice<'a, D> {
    fn read(
        &self,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        let Some(position) = self.get_device_position(absolute_position) else {
            return core::task::Poll::Ready(Ok(0));
        };
        let length = self.get_total_buffer_length(absolute_position, buffer.len());
        DirectBaseOperations::read(self.base_device, cx, &mut buffer[..length], position)
    }

    fn write(
        &self,
        cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        let Some(position) = self.get_device_position(absolute_position) else {
            return core::task::Poll::Ready(Ok(0));
        };
        let length = self.get_total_buffer_length(absolute_position, buffer.len());
        DirectBaseOperations::write(self.base_device, cx, &buffer[..length], position)
    }

    fn write_pattern(
        &self,
        cx: &mut core::task::Context<'_>,
        pattern: &[u8],
        count: usize,
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        let Some(position) = self.get_device_position(absolute_position) else {
            return core::task::Poll::Ready(Ok(0));
        };
        if pattern.is_empty() {
            return core::task::Poll::Ready(Ok(0));
        }
        let count = count.min(
            self.get_total_buffer_length(absolute_position, pattern.len().saturating_mul(count))
                / pattern.len(),
        );
        DirectBaseOperations::write_pattern(self.base_device, cx, pattern, count, position)
    }

    fn set_position(&self, current_position: Size, position: &Position) -> Result<Size> {
        let position = block_device::set_position(current_position, position, self.size)?;

        let device_position = self
            .get_device_position(position)
            .ok_or(Error::InvalidParameter)?;

        self.base_device
            .set_position(device_position, &Position::Start(0))?;

        Ok(position)
    }

    fn flush(&self, cx: &mut core::task::Context<'_>) -> core::task::Poll<Result<()>> {
        DirectBaseOperations::flush(self.base_device, cx)
    }

    fn control(
        &self,
        cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        match command {
            GET_BLOCK_SIZE::IDENTIFIER => {
                *GET_BLOCK_SIZE::cast_output(output)? = self.block_size;
                core::task::Poll::Ready(Ok(()))
            }
            GET_BLOCK_COUNT::IDENTIFIER => {
                *GET_BLOCK_COUNT::cast_output(output)? = self.get_block_count();
                core::task::Poll::Ready(Ok(()))
            }
            ERASE::IDENTIFIER => {
                // Ranges are relative to the partition, never to the base device.
                let range = match ERASE::cast_input(input) {
                    Ok(range) => *range,
                    Err(error) => return core::task::Poll::Ready(Err(error)),
                };
                let Some(position) = range
                    .position
                    .checked_add(range.size)
                    .filter(|end| *end <= self.size)
                    .and_then(|_| self.offset.checked_add(range.position))
                else {
                    return core::task::Poll::Ready(Err(Error::InvalidParameter));
                };
                let range = EraseRange {
                    position,
                    size: range.size,
                };
                DirectBaseOperations::control(
                    self.base_device,
                    cx,
                    command,
                    AnyByLayout::from(&range),
                    output,
                )
            }
            _ => DirectBaseOperations::control(self.base_device, cx, command, input, output),
        }
    }
    fn cancel_operation(&self, operation: crate::Operation) {
        DirectBaseOperations::cancel_operation(self.base_device, operation);
    }
}

impl<'a, D: MountOperations> MountOperations for PartitionDevice<'a, D> {}

impl<'a, D: DirectBlockDevice> DirectBlockDevice for PartitionDevice<'a, D> {}

impl<'a, D> fmt::Debug for PartitionDevice<'a, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PartitionDevice")
            .field("block_size", &self.block_size)
            .field("offset", &self.offset)
            .field("size", &self.size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use crate::{MemoryDevice, implement_block_device_tests};

    use super::*;

    struct Deferred(core::sync::atomic::AtomicUsize);
    impl DirectBaseOperations for Deferred {
        fn read(
            &self,
            _: &mut core::task::Context<'_>,
            _: &mut [u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            core::task::Poll::Pending
        }
        fn write(
            &self,
            _: &mut core::task::Context<'_>,
            _: &[u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            core::task::Poll::Pending
        }
        fn cancel_operation(&self, _: crate::Operation) {
            self.0.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
    /// Records the last erased range of the base device.
    struct Erasable(core::cell::Cell<Option<EraseRange>>);
    // SAFETY: tests are single-threaded.
    unsafe impl Sync for Erasable {}
    impl DirectBaseOperations for Erasable {
        fn read(
            &self,
            _: &mut core::task::Context<'_>,
            _: &mut [u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            core::task::Poll::Ready(Ok(0))
        }
        fn write(
            &self,
            _: &mut core::task::Context<'_>,
            _: &[u8],
            _: Size,
        ) -> core::task::Poll<Result<usize>> {
            core::task::Poll::Ready(Ok(0))
        }
        fn control(
            &self,
            _: &mut core::task::Context<'_>,
            command: ControlCommandIdentifier,
            input: &AnyByLayout,
            _: &mut AnyByLayout,
        ) -> core::task::Poll<Result<()>> {
            crate::ready(|| match command {
                ERASE::IDENTIFIER => {
                    self.0.set(Some(*ERASE::cast_input(input)?));
                    Ok(())
                }
                _ => Err(Error::UnsupportedOperation),
            })
        }
    }
    impl MountOperations for Erasable {}
    impl DirectBlockDevice for Erasable {}

    #[test]
    fn partition_erases_only_inside_its_own_range() {
        let base = Erasable(core::cell::Cell::new(None));
        let partition = PartitionDevice::new(&base, 4, 2, 512);

        partition.erase(512, 512).unwrap();
        assert_eq!(
            base.0.take(),
            Some(EraseRange {
                position: 5 * 512,
                size: 512
            })
        );

        assert_eq!(partition.erase(512, 1024), Err(Error::InvalidParameter));
        assert_eq!(partition.erase(u64::MAX, 2), Err(Error::InvalidParameter));
        assert_eq!(base.0.take(), None);
    }

    #[test]
    fn rewritable_device_reports_erase_as_unsupported() {
        let base = MemoryDevice::<512>::new(4096);
        assert_eq!(base.erase(0, 512), Err(Error::UnsupportedOperation));
    }

    #[test]
    fn partition_forwards_pending_and_cancellation_to_its_device() {
        let device = Deferred(core::sync::atomic::AtomicUsize::new(0));
        let partition = PartitionDevice::new(&device, 0, 1, 512);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        assert!(DirectBaseOperations::read(&partition, &mut cx, &mut [0], 0).is_pending());
        DirectBaseOperations::cancel_operation(&partition, crate::Operation::Read);
        assert_eq!(device.0.load(core::sync::atomic::Ordering::Relaxed), 1);
    }

    implement_block_device_tests!(PartitionDevice::new(
        Box::leak(Box::new(MemoryDevice::<512>::new(1024 * 1024))),
        0,
        1024 * 512,
        512
    ));
}
