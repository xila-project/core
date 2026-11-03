//! Std test devices: time, randomness and console.

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

/// Bootstrap epoch source. Host wall-clock changes require explicit resynchronization.
impl time::RealtimeSource for TimeDevice {
    fn read(&self) -> time::Result<time::RealtimeSample> {
        let now = SystemTime::now();
        let timestamp = match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => time::Timestamp::from(duration),
            Err(before) => time::Timestamp::from_nanos(-(before.duration().as_nanos() as i128)),
        };
        Ok(time::RealtimeSample {
            timestamp,
            source: time::ClockSource::Host,
            // Portable conservative quantization; actual host accuracy is unknown.
            resolution: Duration::from_millis(1),
            accuracy: None,
        })
    }
}

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

/// Standard input, output or error of the test process. Never a terminal,
/// so tests behave the same whether or not they run interactively.
pub enum ConsoleDevice {
    Input,
    Output,
    Error,
}

impl DirectBaseOperations for ConsoleDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        use std::io::Read;
        file_system::ready(|| match self {
            Self::Input => std::io::stdin()
                .read(buffer)
                .map_err(|_| Error::InputOutput),
            _ => Err(Error::UnsupportedOperation),
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        use std::io::Write;
        file_system::ready(|| match self {
            Self::Input => Err(Error::UnsupportedOperation),
            Self::Output => std::io::stdout()
                .write(buffer)
                .map_err(|_| Error::InputOutput),
            Self::Error => std::io::stderr()
                .write(buffer)
                .map_err(|_| Error::InputOutput),
        })
    }

    fn flush(
        &self,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<file_system::Result<()>> {
        use std::io::Write;
        file_system::ready(|| match self {
            Self::Input => Ok(()),
            Self::Output => std::io::stdout().flush().map_err(|_| Error::InputOutput),
            Self::Error => std::io::stderr().flush().map_err(|_| Error::InputOutput),
        })
    }
}

impl MountOperations for ConsoleDevice {}

impl DirectCharacterDevice for ConsoleDevice {}
