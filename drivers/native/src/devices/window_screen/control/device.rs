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
    response_frame: ResponseAccumulator,
}

#[derive(Default)]
struct ResponseAccumulator {
    bytes: Vec<u8>,
    expected_len: Option<usize>,
}

impl ResponseAccumulator {
    fn push(&mut self, bytes: &[u8]) -> file_system::Result<bool> {
        let max_response = debug_protocol::MAX_RESPONSE_BYTES
            .checked_add(debug_protocol::MAX_SCREENSHOT_BYTES)
            .ok_or(file_system::Error::InvalidParameter)?;
        if self.bytes.len().saturating_add(bytes.len()) > max_response {
            return Err(file_system::Error::InvalidParameter);
        }
        self.bytes.extend_from_slice(bytes);

        if self.expected_len.is_none() {
            let Some(header_end) = self.bytes.iter().position(|byte| *byte == b'\n') else {
                if self.bytes.len() >= debug_protocol::MAX_RESPONSE_BYTES {
                    return Err(file_system::Error::InvalidParameter);
                }
                return Ok(false);
            };
            if header_end + 1 > debug_protocol::MAX_RESPONSE_BYTES {
                return Err(file_system::Error::InvalidParameter);
            }
            let header: serde_json::Value = serde_json::from_slice(&self.bytes[..header_end])
                .map_err(|_| file_system::Error::InvalidParameter)?;
            let screenshot = header
                .get("result")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|result| result.contains_key("format"));
            let payload_bytes = if screenshot {
                let metadata: debug_protocol::ScreenshotMetadata = serde_json::from_value(
                    header
                        .get("result")
                        .cloned()
                        .ok_or(file_system::Error::InvalidParameter)?,
                )
                .map_err(|_| file_system::Error::InvalidParameter)?;
                debug_protocol::validate_screenshot_metadata(metadata)
                    .map_err(|_| file_system::Error::InvalidParameter)?;
                metadata.payload_bytes
            } else {
                0
            };
            self.expected_len = Some(
                header_end
                    .checked_add(1)
                    .and_then(|length| length.checked_add(payload_bytes))
                    .ok_or(file_system::Error::InvalidParameter)?,
            );
        }

        let expected = self.expected_len.unwrap();
        if self.bytes.len() > expected {
            return Err(file_system::Error::InvalidParameter);
        }
        Ok(self.bytes.len() == expected)
    }
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

    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
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
                            response_frame: ResponseAccumulator::default(),
                        });
                    }
                    Err(TryRecvError::Empty) => return Err(file_system::Error::RessourceBusy),
                    Err(TryRecvError::Disconnected) => {
                        return Err(file_system::Error::InputOutput);
                    }
                }
            }

            let request = active.as_mut().unwrap();
            if request.offset == request.bytes.len() {
                return Err(file_system::Error::RessourceBusy);
            }
            let length = buffer.len().min(request.bytes.len() - request.offset);
            buffer[..length]
                .copy_from_slice(&request.bytes[request.offset..request.offset + length]);
            request.offset += length;
            Ok(length)
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let state = Self::state()?;
            let mut active = state
                .active
                .try_lock()
                .map_err(|_| file_system::Error::RessourceBusy)?;
            let request = active.as_mut().ok_or(file_system::Error::RessourceBusy)?;
            let length = buffer.len();
            if request.response_frame.push(buffer)? {
                let active = active.take().unwrap();
                active
                    .response
                    .send(active.response_frame.bytes)
                    .map_err(|_| file_system::Error::InputOutput)?;
            }
            Ok(length)
        })
    }
}

impl MountOperations for DebugTransportDevice {}
impl DirectCharacterDevice for DebugTransportDevice {}

#[cfg(test)]
mod tests {
    use super::*;

    fn screenshot_header(payload_bytes: usize) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "ok": true,
            "result": {
                "width": 1,
                "height": 1,
                "format": "rgba8888",
                "payload_bytes": payload_bytes,
            }
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn screenshot_frame_rejects_payload_length_mismatch() {
        let mut frame = ResponseAccumulator::default();
        assert_eq!(
            frame.push(&screenshot_header(3)),
            Err(file_system::Error::InvalidParameter)
        );
    }

    #[test]
    fn screenshot_frame_handles_embedded_newlines() {
        let mut frame = ResponseAccumulator::default();
        let mut bytes = screenshot_header(4);
        bytes.extend_from_slice(&[1, b'\n', 3, 4]);
        assert_eq!(frame.push(&bytes), Ok(true));
        assert_eq!(&frame.bytes[frame.bytes.len() - 4..], &[1, b'\n', 3, 4]);
    }

    #[test]
    fn screenshot_frame_handles_partial_io() {
        let mut frame = ResponseAccumulator::default();
        let header = screenshot_header(4);
        assert_eq!(frame.push(&header[..5]), Ok(false));
        assert_eq!(frame.push(&header[5..]), Ok(false));
        assert_eq!(frame.push(&[1, b'\n']), Ok(false));
        assert_eq!(frame.push(&[3, 4]), Ok(true));
    }
}
