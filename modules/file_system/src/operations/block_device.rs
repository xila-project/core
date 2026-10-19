use crate::{
    BaseOperations, ControlCommand, DirectBaseOperations, Error, MountOperations, Position, Result,
    Size, define_command,
};
use shared::AnyByLayout;

define_command!(GET_BLOCK_SIZE, Read, b'B', 0, (), u32);
define_command!(GET_BLOCK_COUNT, Read, b'B', 1, (), u32);

pub trait BlockDevice: BaseOperations + MountOperations {}

pub trait DirectBlockDevice: DirectBaseOperations + MountOperations {
    fn get_block_size(&self) -> Result<u32> {
        let mut block_size: u32 = 0;

        self.control_sync(
            GET_BLOCK_SIZE::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut block_size),
        )?;
        Ok(block_size)
    }

    fn get_block_count(&self) -> Result<Size> {
        let mut total_size: Size = 0;

        self.control_sync(
            GET_BLOCK_COUNT::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut total_size),
        )?;
        Ok(total_size)
    }
}

pub fn set_position(current_position: Size, position: &Position, size: Size) -> Result<Size> {
    let new_position = match position {
        Position::Start(position) => (*position)
            .try_into()
            .map_err(|_| Error::InvalidParameter)?,
        Position::Current(offset) => (current_position as i64).wrapping_add(*offset),
        Position::End(offset) => (size as i64).saturating_add(*offset),
    };

    if new_position < 0 || (new_position as u64) > size {
        return Err(Error::InvalidParameter);
    }

    Ok(new_position as Size)
}

impl<T> BlockDevice for T where T: DirectBlockDevice + MountOperations {}

pub mod tests {
    use alloc::vec;

    use crate::{Context, Position};

    use super::*;

    /// Generic test for open and close operations.
    pub fn test_open_and_close<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();
        device.open(&mut context).unwrap();
        device.close(&mut context).unwrap();
    }

    /// Generic test for basic read and write operations.
    pub fn test_basic_read_write<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let write_data = b"Hello, Context!";
        let bytes_written = device.write_sync(&mut context, write_data, 0).unwrap();
        assert_eq!(bytes_written, write_data.len());

        let mut read_buffer = vec![0u8; write_data.len()];
        let bytes_read = device.read_sync(&mut context, &mut read_buffer, 0).unwrap();
        assert_eq!(bytes_read, write_data.len());
        assert_eq!(&read_buffer[..], write_data);
    }

    /// Generic test for writing at a specific offset.
    pub fn test_write_at_offset<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let data = b"Xila";
        let offset = 100;
        let bytes_written = device.write_sync(&mut context, data, offset).unwrap();
        assert_eq!(bytes_written, data.len());

        let mut read_buffer = vec![0u8; data.len()];
        let bytes_read = device
            .read_sync(&mut context, &mut read_buffer, offset)
            .unwrap();
        assert_eq!(bytes_read, data.len());
        assert_eq!(&read_buffer[..], data);
    }

    /// Generic test for position management.
    pub fn test_position_management<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let position = 80;

        let position = device
            .set_position(&mut context, position, &Position::Start(50))
            .unwrap();
        assert_eq!(position, 50);

        let position = device
            .set_position(&mut context, position, &Position::Current(25))
            .unwrap();
        assert_eq!(position, 75);
    }

    /// Generic test for write pattern functionality.
    pub fn test_write_pattern<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let pattern = b"XY";
        let pattern_count = 4;
        let position = 200;
        let bytes_written = device
            .write_pattern_sync(&mut context, pattern, pattern_count, position)
            .unwrap();
        assert_eq!(bytes_written, pattern.len() * pattern_count);

        let mut read_buffer = vec![0u8; pattern.len() * pattern_count];
        let bytes_read = device
            .read_sync(&mut context, &mut read_buffer, position)
            .unwrap();
        assert_eq!(bytes_read, pattern.len() * pattern_count);
        assert_eq!(&read_buffer[..], b"XYXYXYXY");
    }

    /// Generic test for vectored write operations.
    pub fn test_write_vectored<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let buffers = [b"Alpha".as_slice(), b"Beta".as_slice()];
        let position = 300;
        let total_written = device
            .write_vectored_sync(&mut context, &buffers, position)
            .unwrap();
        assert_eq!(total_written, 9); // 5 + 4

        let mut read_buffer = vec![0u8; 9];
        let bytes_read = device
            .read_sync(&mut context, &mut read_buffer, position)
            .unwrap();
        assert_eq!(bytes_read, 9);
        assert_eq!(&read_buffer[..], b"AlphaBeta");
    }

    /// Generic test for read until delimiter.
    pub fn test_read_until_delimiter<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        let test_data = b"Start|Middle|End";
        device.write_sync(&mut context, test_data, 400).unwrap();

        let mut read_buffer = vec![0u8; 20];
        let bytes_read = device
            .read_until_sync(&mut context, &mut read_buffer, 400, b"|")
            .unwrap();
        assert_eq!(&read_buffer[..bytes_read], b"Start|");
    }

    /// Generic test for flush operation.
    pub fn test_flush<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();
        device.flush_sync(&mut context).unwrap();
    }

    /// Generic test for context cloning.
    pub fn test_clone_context<T: BlockDevice>(device: &T) {
        let context = Context::new_empty();
        let cloned_context = device.clone_context(&context).unwrap();
        drop(cloned_context);
    }

    /// Generic test for control command.
    pub fn test_control_commands<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();
        let mut block_count: Size = 0;
        device
            .control_sync(
                &mut context,
                GET_BLOCK_COUNT::IDENTIFIER,
                AnyByLayout::NONE,
                AnyByLayout::from_mutable(&mut block_count),
            )
            .unwrap();

        assert!(block_count > 0);

        let mut block_size: usize = 0;
        device
            .control_sync(
                &mut context,
                GET_BLOCK_SIZE::IDENTIFIER,
                AnyByLayout::NONE,
                AnyByLayout::from_mutable(&mut block_size),
            )
            .unwrap();

        assert!(block_size > 0);
    }

    /// Generic test for read until boundary conditions.
    pub fn test_read_until_boundary_conditions<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        // Test: delimiter not found
        let test_data = b"NoDelimiterHere";
        device.write_sync(&mut context, test_data, 0).unwrap();

        let mut read_buffer = vec![0u8; test_data.len()];
        let bytes_read = device
            .read_until_sync(&mut context, &mut read_buffer, 0, b"|")
            .unwrap();
        assert_eq!(bytes_read, test_data.len());
        assert_eq!(&read_buffer[..], test_data);

        // Test: delimiter at start
        let test_data = b"|Start";
        device.write_sync(&mut context, test_data, 100).unwrap();

        let mut read_buffer = vec![0u8; 10];
        let bytes_read = device
            .read_until_sync(&mut context, &mut read_buffer, 100, b"|")
            .unwrap();
        assert_eq!(&read_buffer[..bytes_read], b"|");

        // Test: partial delimiter match followed by different character
        let test_data = b"ABACD";
        device.write_sync(&mut context, test_data, 200).unwrap();

        let mut read_buffer = vec![0u8; test_data.len()];
        let bytes_read = device
            .read_until_sync(&mut context, &mut read_buffer, 200, b"ABC")
            .unwrap();
        assert_eq!(bytes_read, test_data.len());
        assert_eq!(&read_buffer[..], test_data);
    }

    /// Generic test for write pattern edge cases.
    pub fn test_write_pattern_edge_cases<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        // Test: zero count
        let pattern = b"XYZ";
        let bytes_written = device
            .write_pattern_sync(&mut context, pattern, 0, 0)
            .unwrap();
        assert_eq!(bytes_written, 0);

        // Test: single byte pattern
        let pattern = b"A";
        let bytes_written = device
            .write_pattern_sync(&mut context, pattern, 10, 0)
            .unwrap();
        assert_eq!(bytes_written, 10);

        let mut read_buffer = vec![0u8; 10];
        device.read_sync(&mut context, &mut read_buffer, 0).unwrap();
        assert_eq!(&read_buffer[..], b"AAAAAAAAAA");
    }

    /// Generic test for write vectored edge cases.
    pub fn test_write_vectored_edge_cases<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        // Test: empty buffers array
        let buffers: [&[u8]; 0] = [];
        let bytes_written = device
            .write_vectored_sync(&mut context, &buffers, 0)
            .unwrap();
        assert_eq!(bytes_written, 0);

        // Test: buffers with empty slices
        let buffers = [b"First".as_slice(), b"".as_slice(), b"Third".as_slice()];
        let bytes_written = device
            .write_vectored_sync(&mut context, &buffers, 0)
            .unwrap();
        assert_eq!(bytes_written, buffers.into_iter().map(|b| b.len()).sum());

        let mut read_buffer = vec![0u8; 10];
        device.read_sync(&mut context, &mut read_buffer, 0).unwrap();
        assert_eq!(&read_buffer[..], b"FirstThird");
    }

    /// Generic test for position wraparound scenarios.
    pub fn test_position_wraparound<T: BlockDevice>(device: &T, device_size: Size) {
        let mut context = Context::new_empty();

        let position = 0;
        // Set to near end
        let near_end = device_size.saturating_sub(100);
        let position = device
            .set_position(&mut context, position, &Position::Start(near_end))
            .unwrap();
        assert_eq!(position, near_end);

        // Move forward from current
        let position = device
            .set_position(&mut context, position, &Position::Current(50))
            .unwrap();
        assert_eq!(position, near_end + 50);

        // Move backward from current
        let position = device
            .set_position(&mut context, position, &Position::Current(-100))
            .unwrap();
        assert_eq!(position, near_end.saturating_sub(50));

        // Position from end
        let position = device
            .set_position(&mut context, position, &Position::End(-50))
            .unwrap();
        assert_eq!(position, device_size - 50);

        // Position exactly at end
        let position = device
            .set_position(&mut context, position, &Position::End(0))
            .unwrap();
        assert_eq!(position, device_size);
    }

    /// Generic test for concurrent operations.
    pub fn test_concurrent_operations<T: BlockDevice + Sync>(device: &T) {
        let mut context = Context::new_empty();
        let mut context_clone = Context::new_empty();

        // Write from one context
        let write_data = b"Concurrent Write Test";
        device.write_sync(&mut context, write_data, 0).unwrap();

        // Read from another context
        let mut read_buffer = vec![0u8; write_data.len()];
        device
            .read_sync(&mut context_clone, &mut read_buffer, 0)
            .unwrap();
        assert_eq!(&read_buffer[..], write_data);
    }

    /// Generic test for large read/write operations.
    pub fn test_large_read_write<T: BlockDevice>(device: &T) {
        let mut context = Context::new_empty();

        // Test with a large buffer
        let large_data = vec![0x42u8; 4096];
        let bytes_written = device.write_sync(&mut context, &large_data, 0).unwrap();
        assert_eq!(bytes_written, large_data.len());

        let mut read_buffer = vec![0u8; 4096];
        let bytes_read = device.read_sync(&mut context, &mut read_buffer, 0).unwrap();
        assert_eq!(bytes_read, large_data.len());
        assert_eq!(read_buffer, large_data);
    }

    /// Tests for the set_position function.
    #[cfg(test)]
    mod set_position_tests {
        use super::*;

        #[test]
        fn test_position_start_valid() {
            let result = set_position(100, &Position::Start(50), 1000);
            assert_eq!(result.unwrap(), 50);
        }

        #[test]
        fn test_position_start_at_zero() {
            let result = set_position(100, &Position::Start(0), 1000);
            assert_eq!(result.unwrap(), 0);
        }

        #[test]
        fn test_position_start_at_size() {
            let result = set_position(100, &Position::Start(1000), 1000);
            assert_eq!(result.unwrap(), 1000);
        }

        #[test]
        fn test_position_start_exceeds_size() {
            let result = set_position(100, &Position::Start(1001), 1000);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), Error::InvalidParameter);
        }

        #[test]
        fn test_position_current_positive_offset() {
            let result = set_position(100, &Position::Current(50), 1000);
            assert_eq!(result.unwrap(), 150);
        }

        #[test]
        fn test_position_current_negative_offset() {
            let result = set_position(100, &Position::Current(-50), 1000);
            assert_eq!(result.unwrap(), 50);
        }

        #[test]
        fn test_position_current_zero_offset() {
            let result = set_position(100, &Position::Current(0), 1000);
            assert_eq!(result.unwrap(), 100);
        }

        #[test]
        fn test_position_current_exceeds_size() {
            let result = set_position(100, &Position::Current(901), 1000);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), Error::InvalidParameter);
        }

        #[test]
        fn test_position_current_wraps_to_valid() {
            // Test wraparound behavior
            let result = set_position(Size::MAX - 50, &Position::Current(100), Size::MAX);
            assert_eq!(result.unwrap(), Size::MAX.wrapping_add(50));
        }

        #[test]
        fn test_position_end_zero_offset() {
            let result = set_position(100, &Position::End(0), 1000);
            assert_eq!(result.unwrap(), 1000);
        }

        #[test]
        fn test_position_end_negative_offset() {
            let result = set_position(100, &Position::End(-50), 1000);
            assert_eq!(result.unwrap(), 950);
        }

        #[test]
        fn test_position_end_positive_offset() {
            let result = set_position(100, &Position::End(10), 1000);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), Error::InvalidParameter);
        }

        #[test]
        fn test_position_end_negative_exceeds_zero() {
            let result = set_position(100, &Position::End(-1001), 1000);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), Error::InvalidParameter);
        }

        #[test]
        fn test_position_end_exactly_at_start() {
            let result = set_position(100, &Position::End(-1000), 1000);
            assert_eq!(result.unwrap(), 0);
        }

        #[test]
        fn test_position_boundaries() {
            // Test at exact boundaries
            let size = 1000;

            // Start boundary
            assert_eq!(set_position(0, &Position::Start(0), size).unwrap(), 0);
            assert_eq!(set_position(0, &Position::Start(size), size).unwrap(), size);

            // End boundary
            assert_eq!(set_position(0, &Position::End(0), size).unwrap(), size);
            assert_eq!(
                set_position(0, &Position::End(-(size as i64)), size).unwrap(),
                0
            );

            // Current boundary
            assert_eq!(set_position(0, &Position::Current(0), size).unwrap(), 0);
            assert_eq!(
                set_position(0, &Position::Current(size as i64), size).unwrap(),
                size
            );
        }
    }

    #[macro_export]
    macro_rules! implement_block_device_tests {
        ($device:expr) => {
            #[test]
            fn test_open_close() {
                let device = $device;
                $crate::block_device::tests::test_open_and_close(&device);
            }

            #[test]
            fn test_basic_read_write() {
                let device = $device;
                 $crate::block_device::tests::test_basic_read_write(&device);
            }

            #[test]
            fn test_write_at_offset() {
                let device = $device;
                 $crate::block_device::tests::test_write_at_offset(&device);
            }

            #[test]
            fn test_position_management() {
                let device = $device;
                 $crate::block_device::tests::test_position_management(&device);
            }

            #[test]
            fn test_write_pattern() {
                let device = $device;
                 $crate::block_device::tests::test_write_pattern(&device);
            }

            #[test]
            fn test_write_vectored() {
                let device = $device;
                 $crate::block_device::tests::test_write_vectored(&device);
            }

            #[test]
            fn test_read_until_delimiter() {
                let device = $device;
                 $crate::block_device::tests::test_read_until_delimiter(&device);
            }

            #[test]
            fn test_flush() {
                let device = $device;
                 $crate::block_device::tests::test_flush(&device);
            }

            #[test]
            fn test_clone_context() {
                let device = $device;
                 $crate::block_device::tests::test_clone_context(&device);
            }

            #[test]
            fn test_control_commands() {
                let device = $device;
                 $crate::block_device::tests::test_control_commands(&device);
            }

            #[test]
            fn test_read_until_boundary_conditions() {
                let device = $device;
                 $crate::block_device::tests::test_read_until_boundary_conditions(&device);
            }

            #[test]
            fn test_write_pattern_edge_cases() {
                let device = $device;
                 $crate::block_device::tests::test_write_pattern_edge_cases(&device);
            }

            #[test]
            fn test_write_vectored_edge_cases() {
                let device = $device;
                 $crate::block_device::tests::test_write_vectored_edge_cases(&device);
            }

            #[test]
            fn test_concurrent_operations() {
                let device = $device;
                 $crate::block_device::tests::test_concurrent_operations(&device);
            }

            #[test]
            fn test_large_read_write() {
                let device = $device;
                 $crate::block_device::tests::test_large_read_write(&device);
            }
        };
        (
            instance: $device:expr,
            size: $size:expr
        ) => {
            implement!(instance: $device);

            #[test]
            fn test_position_wraparound() {
                let device = $device;
                test_position_wraparound(&device, $size);
            }
        };
    }
}
