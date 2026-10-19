use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{InputData, Key, State};
use synchronization::{
    blocking_mutex::raw::{CriticalSectionRawMutex, RawMutex},
    channel::{Receiver, Sender},
};

pub struct KeyboardDevice<M, const N: usize>
where
    M: RawMutex + 'static,
{
    receiver: Receiver<'static, M, (Key, State), N>,
    sender: Sender<'static, M, (Key, State), N>,
}

impl<M, const N: usize> KeyboardDevice<M, N>
where
    M: RawMutex + 'static,
{
    pub fn new(
        receiver: Receiver<'static, M, (Key, State), N>,
        sender: Sender<'static, M, (Key, State), N>,
    ) -> Self {
        Self { receiver, sender }
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
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            if buffer.len() != 5 {
                return Err(file_system::Error::InvalidParameter);
            }
            let key_code = u32::from_le_bytes(buffer[..4].try_into().unwrap());
            let key = Key::from(key_code);
            if u32::from(key) != key_code
                || matches!(key, Key::Character(character) if !(b' '..=b'~').contains(&character))
            {
                return Err(file_system::Error::InvalidParameter);
            }
            let state = State::try_from(buffer[4]).map_err(|_| file_system::Error::InvalidParameter)?;
            self.sender.try_send((key, state)).map_err(|_| file_system::Error::RessourceBusy)?;
            Ok(buffer.len())
        })
    }
}

impl<const N: usize> MountOperations for KeyboardDevice<CriticalSectionRawMutex, N> {}

impl<const N: usize> DirectCharacterDevice for KeyboardDevice<CriticalSectionRawMutex, N> {}
