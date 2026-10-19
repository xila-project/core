use file_system::{CharacterDevice, DirectBaseOperations, MountOperations, Size};

pub struct NullDevice;

impl DirectBaseOperations for NullDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Ok(0))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Ok(buffer.len() as _))
    }
}

impl MountOperations for NullDevice {}

impl CharacterDevice for NullDevice {}
