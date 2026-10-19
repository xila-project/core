use std::sync::{
    Arc, Mutex, OnceLock,
    mpsc::{Receiver, TryRecvError},
};

use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};

use super::transport::{ControlServer, PendingRequest};

struct ActiveRequest {
    bytes: Vec<u8>,
    offset: usize,
    response: std::sync::mpsc::Sender<Vec<u8>>,
    response_bytes: Vec<u8>,
}

struct DeviceState {
    requests: Mutex<Receiver<PendingRequest>>,
    active: Mutex<Option<ActiveRequest>>,
}

static DEVICE_STATE: OnceLock<Mutex<Option<Arc<DeviceState>>>> = OnceLock::new();
static CONTROL_SERVER: Mutex<Option<ControlServer>> = Mutex::new(None);

/// VFS device through which the Debug application receives agent requests and replies.
#[derive(Clone, Copy, Debug, Default)]
pub struct DebugTransportDevice;

impl DebugTransportDevice {
    fn initialize() -> file_system::Result<()> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let state = Arc::new(DeviceState {
            requests: Mutex::new(receiver),
            active: Mutex::new(None),
        });
        let state_slot = DEVICE_STATE.get_or_init(|| Mutex::new(None));
        let mut state_guard = state_slot
            .lock()
            .map_err(|_| file_system::Error::InputOutput)?;
        if state_guard.is_some() {
            return Err(file_system::Error::RessourceBusy);
        }
        let server = ControlServer::start(sender).map_err(|_| file_system::Error::InputOutput)?;
        *CONTROL_SERVER
            .lock()
            .map_err(|_| file_system::Error::InputOutput)? = Some(server);
        *state_guard = Some(state);
        Ok(())
    }

    fn state() -> file_system::Result<Arc<DeviceState>> {
        DEVICE_STATE
            .get()
            .ok_or(file_system::Error::NotInitialized)?
            .lock()
            .map_err(|_| file_system::Error::InputOutput)?
            .clone()
            .ok_or(file_system::Error::NotInitialized)
    }
}

impl DirectBaseOperations for DebugTransportDevice {
    fn open(&self) -> file_system::Result<()> {
        Self::initialize()
    }

    fn close(&self) -> file_system::Result<()> {
        *DEVICE_STATE
            .get()
            .ok_or(file_system::Error::NotInitialized)?
            .lock()
            .map_err(|_| file_system::Error::InputOutput)? = None;
        *CONTROL_SERVER
            .lock()
            .map_err(|_| file_system::Error::InputOutput)? = None;
        Ok(())
    }

    fn read(&self, buffer: &mut [u8], _: Size) -> file_system::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let state = Self::state()?;
        let mut active = state
            .active
            .try_lock()
            .map_err(|_| file_system::Error::RessourceBusy)?;

        if active.is_none() {
            let requests = state
                .requests
                .try_lock()
                .map_err(|_| file_system::Error::RessourceBusy)?;
            match requests.try_recv() {
                Ok(request) => {
                    *active = Some(ActiveRequest {
                        bytes: request.bytes,
                        offset: 0,
                        response: request.response,
                        response_bytes: Vec::new(),
                    });
                }
                Err(TryRecvError::Empty) => return Err(file_system::Error::RessourceBusy),
                Err(TryRecvError::Disconnected) => return Err(file_system::Error::InputOutput),
            }
        }

        let request = active.as_mut().unwrap();
        if request.offset == request.bytes.len() {
            return Err(file_system::Error::RessourceBusy);
        }
        let length = buffer.len().min(request.bytes.len() - request.offset);
        buffer[..length].copy_from_slice(&request.bytes[request.offset..request.offset + length]);
        request.offset += length;
        Ok(length)
    }

    fn write(&self, buffer: &[u8], _: Size) -> file_system::Result<usize> {
        let state = Self::state()?;
        let mut active = state
            .active
            .try_lock()
            .map_err(|_| file_system::Error::RessourceBusy)?;
        let request = active.as_mut().ok_or(file_system::Error::RessourceBusy)?;
        if request.response_bytes.len() + buffer.len() > debug_protocol::MAX_RESPONSE_BYTES {
            return Err(file_system::Error::InvalidParameter);
        }
        request.response_bytes.extend_from_slice(buffer);
        let length = buffer.len();

        if request.response_bytes.contains(&b'\n') {
            let active = active.take().unwrap();
            active
                .response
                .send(active.response_bytes)
                .map_err(|_| file_system::Error::InputOutput)?;
        }
        Ok(length)
    }
}

impl MountOperations for DebugTransportDevice {}
impl DirectCharacterDevice for DebugTransportDevice {}
