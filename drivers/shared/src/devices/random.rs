use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};

pub struct RandomDevice;

impl DirectBaseOperations for RandomDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            getrandom::fill(buffer).map_err(|_| file_system::Error::Other)?;

            Ok(buffer.len())
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }

    fn set_position(
        &self,
        _: Size,
        _position: &file_system::Position,
    ) -> file_system::Result<file_system::Size> {
        Err(file_system::Error::UnsupportedOperation)
    }
}

impl MountOperations for RandomDevice {}

impl DirectCharacterDevice for RandomDevice {}
