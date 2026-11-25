//! Human interface emulation device node (`/devices/human_interface/emulation`).
//!
//! [`HumanInterfaceHandle`] adapts a `peripherals::human_interface::HumanInterfaceController`
//! to [`PeripheralHandle`]. The node is write-only for reports: `read` fails with
//! `Unsupported`, and the connection status is the [`GET_STATUS`] control command, which
//! WebAssembly guests reach through the `fd_control` import of the Wasm host.
//!
//! Applications write 8-byte messages (`device::human_interface::Message`): each `write`
//! holds one or more messages, a write whose length is not a multiple of 8 or that contains
//! an unknown kind fails with `InvalidArgument`. The handle validates every message, converts
//! it to its HID report with `peripherals::human_interface::Report::from_message` and calls
//! `send_report`; all validation and error handling live here, not in the transports.
//!
//! # Control commands (kind `b'h'`)
//!
//! | Number | Command     | Direction | Input        | Output      | Effect                                                      |
//! |--------|-------------|-----------|--------------|-------------|-------------------------------------------------------------|
//! | `0x00` | `GET_STATUS` | Read      | `VersionAbi` | `StatusAbi` | Returns the connection state and the keyboard LED bitmask. |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and carries zeroed
//! `reserved` fields. A wrong version or a non-zero reserved field is `InvalidArgument`.

use file_system::{ControlCommand, ControlCommandIdentifier, IoErrorKind, Result};
use peripherals::human_interface::{Error as ControllerError, HumanInterfaceController, Report};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::human_interface::{
    ABI_VERSION, EMULATION_PATH, GET_STATUS, INDICATOR_CAPS_LOCK, INDICATOR_NUM_LOCK,
    INDICATOR_SCROLL_LOCK, KEY_COUNT, KIND_CONSUMER, KIND_KEYBOARD, KIND_MOUSE, KIND_SYSTEM,
    MESSAGE_SIZE, Message, Messages, Modifiers, MouseButtons, StatusAbi, VersionAbi, consumer,
    keyboard, system,
};

/// Adapts a human interface controller to [`PeripheralHandle`].
pub struct HumanInterfaceHandle<C>(C);

impl<C: HumanInterfaceController> HumanInterfaceHandle<C> {
    pub const fn new(controller: C) -> Self {
        Self(controller)
    }
}

impl<C> PeripheralHandle for HumanInterfaceHandle<C>
where
    C: HumanInterfaceController + Send,
{
    fn read(&mut self, _buffer: &mut [u8]) -> Result<usize> {
        // Reports are write-only; the status is the GET_STATUS control command.
        Err(IoErrorKind::Unsupported)
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        // Validate the whole write first: one invalid message rejects the write before any
        // report is forwarded.
        for message in Messages::new(buffer).map_err(|_| IoErrorKind::InvalidArgument)? {
            message.map_err(|_| IoErrorKind::InvalidArgument)?;
        }
        for message in Messages::new(buffer).map_err(|_| IoErrorKind::InvalidArgument)? {
            let message = message.map_err(|_| IoErrorKind::InvalidArgument)?;
            let report = Report::from_message(&message);
            self.0
                .send_report(report.identifier(), report.bytes())
                .map_err(map_error)?;
        }
        Ok(buffer.len())
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == GET_STATUS::IDENTIFIER {
            let request = GET_STATUS::cast_input(input)?;
            if request.version != ABI_VERSION || request.reserved != 0 {
                return Err(IoErrorKind::InvalidArgument);
            }
            *GET_STATUS::cast_output(output)? = StatusAbi {
                version: ABI_VERSION,
                connected: self.0.is_connected() as u8,
                indicators: self.0.indicators(),
                reserved: [0; 4],
            };
            Ok(())
        } else {
            Err(IoErrorKind::Unsupported)
        }
    }
}

/// Maps a controller error to the file system error seen by applications.
fn map_error(error: ControllerError) -> IoErrorKind {
    match error {
        ControllerError::NotConnected => IoErrorKind::NotConnected,
        ControllerError::Other => IoErrorKind::InputOutput,
    }
}

/// Mounts `controller` at `/devices/human_interface/<name>` (for example `emulation`),
/// creating `/devices` and `/devices/human_interface` when they do not exist. `name` must be
/// a single path component.
#[cfg(feature = "vfs")]
pub async fn mount_human_interface<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    name: &str,
    controller: C,
) -> virtual_file_system::Result<()>
where
    C: HumanInterfaceController + Send + 'static,
{
    use file_system::Path;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(IoErrorKind::InvalidPath);
    }
    let path = Path::HUMAN_INTERFACE_DEVICES
        .join(name)
        .ok_or(IoErrorKind::InvalidPath)?;
    let handle = HumanInterfaceHandle::new(controller);

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::HUMAN_INTERFACE_DEVICES)
            .await,
    )?;

    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use device::human_interface::INDICATOR_NUM_LOCK;
    use peripherals::human_interface::{
        REPORT_ID_CONSUMER, REPORT_ID_KEYBOARD, REPORT_ID_MOUSE, REPORT_ID_SYSTEM,
    };

    /// Controller that records reports and can fail with a chosen error.
    struct RecordingController {
        reports: [Option<(u8, [u8; 8], usize)>; 4],
        count: usize,
        connected: bool,
        indicators: u8,
        fail_with: Option<ControllerError>,
    }

    impl RecordingController {
        fn new() -> Self {
            Self {
                reports: [None; 4],
                count: 0,
                connected: true,
                indicators: 0,
                fail_with: None,
            }
        }
    }

    impl HumanInterfaceController for RecordingController {
        fn send_report(
            &mut self,
            identifier: u8,
            report: &[u8],
        ) -> core::result::Result<(), ControllerError> {
            if let Some(error) = self.fail_with {
                return Err(error);
            }
            let mut bytes = [0u8; 8];
            bytes[..report.len()].copy_from_slice(report);
            self.reports[self.count % 4] = Some((identifier, bytes, report.len()));
            self.count += 1;
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.connected
        }

        fn indicators(&self) -> u8 {
            self.indicators
        }
    }

    fn control<C: ControlCommand>(
        handle: &mut HumanInterfaceHandle<RecordingController>,
        input: &C::Input,
        output: &mut C::Output,
    ) -> Result<()> {
        handle.control(
            C::IDENTIFIER,
            AnyByLayout::from(input),
            AnyByLayout::from_mutable(output),
        )
    }

    #[test]
    fn read_is_unsupported_because_reports_are_write_only() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        assert_eq!(handle.read(&mut [0u8; 8]), Err(IoErrorKind::Unsupported));
    }

    #[test]
    fn writes_forward_every_message_as_a_report() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        let mut buffer = [0u8; 2 * MESSAGE_SIZE];
        buffer[..MESSAGE_SIZE].copy_from_slice(
            &Message::Keyboard {
                modifiers: Modifiers::LEFT_CONTROL,
                keys: [keyboard::A, 0, 0, 0, 0, 0],
            }
            .encode(),
        );
        buffer[MESSAGE_SIZE..].copy_from_slice(
            &Message::Consumer {
                usage: consumer::PLAY_PAUSE,
            }
            .encode(),
        );
        assert_eq!(handle.write(&buffer), Ok(buffer.len()));
        assert_eq!(handle.0.count, 2);
        assert_eq!(
            handle.0.reports[0],
            Some((
                REPORT_ID_KEYBOARD,
                [
                    Modifiers::LEFT_CONTROL.bits(),
                    0,
                    keyboard::A,
                    0,
                    0,
                    0,
                    0,
                    0
                ],
                8
            ))
        );
        assert_eq!(
            handle.0.reports[1],
            Some((REPORT_ID_CONSUMER, [0xCD, 0x00, 0, 0, 0, 0, 0, 0], 2))
        );
    }

    #[test]
    fn all_message_kinds_reach_the_controller() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        for message in [
            Message::Mouse {
                buttons: MouseButtons::RIGHT,
                x: -1,
                y: 2,
                wheel: 3,
                pan: -4,
            },
            Message::System {
                usage: system::SLEEP,
            },
        ] {
            assert_eq!(handle.write(&message.encode()), Ok(MESSAGE_SIZE));
        }
        assert_eq!(handle.0.count, 2);
        assert_eq!(
            handle.0.reports[0],
            Some((REPORT_ID_MOUSE, [0x02, 0xFF, 0x02, 0x03, 0xFC, 0, 0, 0], 5))
        );
        assert_eq!(
            handle.0.reports[1],
            Some((REPORT_ID_SYSTEM, [0x82, 0, 0, 0, 0, 0, 0, 0], 1))
        );
    }

    #[test]
    fn invalid_writes_fail_with_an_invalid_input_error_and_send_nothing() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        // A length that is not a multiple of 8.
        assert_eq!(handle.write(&[0u8; 7]), Err(IoErrorKind::InvalidArgument));
        assert_eq!(handle.write(&[0u8; 9]), Err(IoErrorKind::InvalidArgument));
        // An unknown kind.
        let mut message = Message::System { usage: 0 }.encode();
        message[0] = 77;
        assert_eq!(handle.write(&message), Err(IoErrorKind::InvalidArgument));
        // A valid message followed by an invalid one: nothing is sent, because the
        // write is rejected as a whole before the first report is forwarded.
        let mut buffer = [0u8; 2 * MESSAGE_SIZE];
        buffer[..MESSAGE_SIZE].copy_from_slice(&Message::Consumer { usage: 0 }.encode());
        buffer[MESSAGE_SIZE] = 200;
        assert_eq!(handle.write(&buffer), Err(IoErrorKind::InvalidArgument));
        assert_eq!(handle.0.count, 0);
    }

    #[test]
    fn controller_errors_map_to_file_system_errors() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        handle.0.fail_with = Some(ControllerError::NotConnected);
        assert_eq!(
            handle.write(&Message::System { usage: 0 }.encode()),
            Err(IoErrorKind::NotConnected)
        );
        handle.0.fail_with = Some(ControllerError::Other);
        assert_eq!(
            handle.write(&Message::System { usage: 0 }.encode()),
            Err(IoErrorKind::InputOutput)
        );
    }

    #[test]
    fn get_status_returns_connection_and_indicators() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        handle.0.indicators = INDICATOR_NUM_LOCK;

        let mut status = StatusAbi::default();
        control::<GET_STATUS>(
            &mut handle,
            &VersionAbi {
                version: ABI_VERSION,
                reserved: 0,
            },
            &mut status,
        )
        .unwrap();
        assert_eq!(
            status,
            StatusAbi {
                version: ABI_VERSION,
                connected: 1,
                indicators: INDICATOR_NUM_LOCK,
                reserved: [0; 4],
            }
        );

        handle.0.connected = false;
        control::<GET_STATUS>(
            &mut handle,
            &VersionAbi {
                version: ABI_VERSION,
                reserved: 0,
            },
            &mut status,
        )
        .unwrap();
        assert_eq!(status.connected, 0);
    }

    #[test]
    fn get_status_rejects_a_wrong_version_or_reserved_field() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        let mut status = StatusAbi::default();
        for request in [
            VersionAbi {
                version: ABI_VERSION + 1,
                reserved: 0,
            },
            VersionAbi {
                version: ABI_VERSION,
                reserved: 1,
            },
        ] {
            assert_eq!(
                control::<GET_STATUS>(&mut handle, &request, &mut status),
                Err(IoErrorKind::InvalidArgument)
            );
        }
    }

    #[test]
    fn unknown_commands_are_unsupported() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        let mut output = 0u32;
        assert_eq!(
            handle.control(
                ControlCommandIdentifier::new_read::<(), u32>(b'z', 0),
                AnyByLayout::from(&()),
                AnyByLayout::from_mutable(&mut output),
            ),
            Err(IoErrorKind::Unsupported)
        );
    }
}
