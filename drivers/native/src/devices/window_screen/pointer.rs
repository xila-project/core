use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{InputData, State};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

pub struct PointerDevice(&'static RwLock<CriticalSectionRawMutex, InputData>);

impl PointerDevice {
    pub fn new(signal: &'static RwLock<CriticalSectionRawMutex, InputData>) -> Self {
        Self(signal)
    }
}

impl DirectBaseOperations for PointerDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            // - Cast the pointer data to the buffer.
            let data: &mut InputData = buffer
                .try_into()
                .map_err(|_| file_system::Error::InvalidParameter)?;

            // Copy the pointer data.

            if let Ok(guard) = self.0.try_read() {
                *data = *guard;
            }

            Ok(size_of::<InputData>())
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            if buffer.len() != 6 {
                return Err(file_system::Error::InvalidParameter);
            }
            let x = i16::from_le_bytes([buffer[1], buffer[2]]);
            let y = i16::from_le_bytes([buffer[3], buffer[4]]);
            let state =
                State::try_from(buffer[5]).map_err(|_| file_system::Error::InvalidParameter)?;
            let mut pointer = self
                .0
                .try_write()
                .map_err(|_| file_system::Error::RessourceBusy)?;
            match buffer[0] {
                0 => pointer.set_point(graphics::Point::new(x, y)),
                1 => pointer.set_state(state),
                _ => return Err(file_system::Error::InvalidParameter),
            }
            Ok(buffer.len())
        })
    }
}

impl MountOperations for PointerDevice {}

impl DirectCharacterDevice for PointerDevice {}
