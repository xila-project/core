use core::time::Duration;
use file_system::{
    DirectBaseOperations, DirectCharacterDevice, Error, MountOperations, Result, Size,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TimeDevice;

impl DirectBaseOperations for TimeDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            if buffer.len() < size_of::<Duration>() {
                return Err(Error::InvalidParameter);
            }

            let duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::InternalError)?;

            let duration: Duration = Duration::new(duration.as_secs(), duration.subsec_nanos());

            let duration_bytes = unsafe {
                core::slice::from_raw_parts(
                    &duration as *const Duration as *const u8,
                    size_of::<Duration>(),
                )
            };

            buffer[..duration_bytes.len()].copy_from_slice(duration_bytes);

            Ok(buffer.len())
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }
}

impl MountOperations for TimeDevice {}

impl DirectCharacterDevice for TimeDevice {}
