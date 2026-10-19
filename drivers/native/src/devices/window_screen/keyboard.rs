use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{InputData, Key, State};
use synchronization::{
    blocking_mutex::raw::{CriticalSectionRawMutex, RawMutex},
    channel::Receiver,
};

pub struct KeyboardDevice<M, const N: usize>(Receiver<'static, M, (Key, State), N>)
where
    M: RawMutex + 'static;

impl<M, const N: usize> KeyboardDevice<M, N>
where
    M: RawMutex + 'static,
{
    pub fn new(receiver: Receiver<'static, M, (Key, State), N>) -> Self {
        Self(receiver)
    }
}

impl<const N: usize> DirectBaseOperations for KeyboardDevice<CriticalSectionRawMutex, N> {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            // - Cast
            let data: &mut InputData = buffer
                .try_into()
                .map_err(|_| file_system::Error::InvalidParameter)?;

            let size = if let Ok((key, state)) = self.0.try_receive() {
                data.set_key(key);
                data.set_state(state);
                size_of::<InputData>()
            } else {
                0
            };

            data.set_continue(!self.0.is_empty());

            Ok(size)
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
}

impl<const N: usize> MountOperations for KeyboardDevice<CriticalSectionRawMutex, N> {}

impl<const N: usize> DirectCharacterDevice for KeyboardDevice<CriticalSectionRawMutex, N> {}
