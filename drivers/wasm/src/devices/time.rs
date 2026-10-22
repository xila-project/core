use core::time::Duration;
use file_system::{
    DirectBaseOperations, DirectCharacterDevice, Error, MountOperations, Result, Size,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// JavaScript Date.now() - returns milliseconds since UNIX epoch
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
}

/// WASM Time Device
///
/// Provides access to current time information in WASM environments.
/// Uses JavaScript's Date.now() API to get the current time since UNIX epoch.
///
/// The device returns time as a `Duration` struct containing seconds and nanoseconds
/// since UNIX epoch, similar to the native implementation.
pub struct TimeDevice;

impl Default for TimeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeDevice {
    pub fn new() -> Self {
        Self {}
    }

    /// Get current time as Duration since UNIX epoch
    ///
    /// Uses JavaScript's Date.now() which returns milliseconds since UNIX epoch.
    /// Converts to seconds and nanoseconds for compatibility with Duration.
    fn get_current_time() -> Result<Duration> {
        // Get milliseconds since UNIX epoch from JavaScript Date.now()
        let millis = date_now();

        // Validate that we got a valid timestamp
        if !millis.is_finite() || millis < 0.0 {
            return Err(Error::InternalError);
        }

        // Convert to seconds and nanoseconds
        let secs = (millis / 1000.0) as u64;
        let nanos = ((millis % 1000.0) * 1_000_000.0) as u32;

        Ok(Duration::new(secs, nanos))
    }
}

impl DirectBaseOperations for TimeDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let duration = Self::get_current_time()?;

            let duration_bytes = unsafe {
                core::slice::from_raw_parts(
                    &duration as *const Duration as *const u8,
                    core::mem::size_of::<Duration>(),
                )
            };

            if buffer.len() < duration_bytes.len() {
                return Err(Error::InvalidParameter);
            }

            buffer[..duration_bytes.len()].copy_from_slice(duration_bytes);

            Ok(duration_bytes.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_device_read() {
        let device = TimeDevice::new();
        let mut buffer = [0u8; core::mem::size_of::<Duration>()];

        let result = device.read_sync(&mut buffer, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), core::mem::size_of::<Duration>());
    }

    #[test]
    fn test_time_device_write_unsupported() {
        let device = TimeDevice::new();
        let buffer = [0u8; 8];

        let result = device.write_sync(&buffer, 0);
        assert!(matches!(result, Err(Error::UnsupportedOperation)));
    }
}
