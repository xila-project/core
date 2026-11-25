//! Human interface emulation device node (`/devices/human_interface/emulation`).
//!
//! [`HumanInterfaceHandle`] adapts a `peripherals::human_interface::HumanInterfaceController`
//! to [`PeripheralHandle`]. The node is write-only for reports: `read` fails with
//! `Unsupported`, and the connection status is the [`GET_STATUS`] control command, which
//! WebAssembly guests reach through the `fd_control` import of the Wasm host.
//!
//! Applications write 8-byte messages (`device::human_interface::Message`): each `write`
//! holds one or more messages, and a write whose length is not a multiple of 8 fails with
//! `InvalidArgument`. The handle validates the message structure only (length, kind byte,
//! reserved bytes), converts each message to its HID report with [`Report::from_message`]
//! and calls `send_report` with the report identifier and the report bytes without the
//! identifier byte. All validation and error handling live here, not in the transports:
//! USB drivers prefix the identifier byte themselves, and Bluetooth Low Energy HID over
//! GATT sends it out of band, in the Report Reference descriptor.
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
use peripherals::human_interface::{
    Error as ControllerError, HumanInterfaceController, REPORT_MAX_BYTES,
};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::human_interface::{
    ABI_VERSION, EMULATION_PATH, GET_STATUS, INDICATOR_CAPS_LOCK, INDICATOR_NUM_LOCK,
    INDICATOR_SCROLL_LOCK, KEY_COUNT, KIND_CONSUMER, KIND_KEYBOARD, KIND_MOUSE, KIND_SYSTEM,
    MESSAGE_SIZE, Message, Messages, Modifiers, MouseButtons, StatusAbi, VersionAbi, consumer,
    keyboard, system,
};
pub use peripherals::human_interface::{
    REPORT_DESCRIPTOR, REPORT_IDENTIFIER_CONSUMER, REPORT_IDENTIFIER_KEYBOARD,
    REPORT_IDENTIFIER_MOUSE, REPORT_IDENTIFIER_SYSTEM,
};

/// One HID report: its identifier and its bytes without the identifier byte, matching
/// [`REPORT_DESCRIPTOR`]. USB lower halves prefix the identifier byte; Bluetooth Low Energy
/// sends it out of band.
pub struct Report {
    identifier: u8,
    bytes: [u8; REPORT_MAX_BYTES],
    length: usize,
}

impl Report {
    /// Convert a message to its report, byte for byte as [`REPORT_DESCRIPTOR`] declares it.
    pub fn from_message(message: &Message) -> Self {
        let mut bytes = [0u8; REPORT_MAX_BYTES];
        let (identifier, length) = match message {
            Message::Keyboard { modifiers, keys } => {
                bytes[0] = modifiers.bits();
                bytes[2..2 + KEY_COUNT].copy_from_slice(keys);
                (REPORT_IDENTIFIER_KEYBOARD, 8)
            }
            Message::Consumer { usage } => {
                bytes[..2].copy_from_slice(&usage.to_le_bytes());
                (REPORT_IDENTIFIER_CONSUMER, 2)
            }
            Message::Mouse {
                buttons,
                x,
                y,
                wheel,
                pan,
            } => {
                bytes[0] = buttons.bits() & MouseButtons::ALL.bits();
                bytes[1] = *x as u8;
                bytes[2] = *y as u8;
                bytes[3] = *wheel as u8;
                bytes[4] = *pan as u8;
                (REPORT_IDENTIFIER_MOUSE, 5)
            }
            Message::System { usage } => {
                bytes[0] = *usage;
                (REPORT_IDENTIFIER_SYSTEM, 1)
            }
        };
        Self {
            identifier,
            bytes,
            length,
        }
    }

    /// Report identifier, one of the `REPORT_IDENTIFIER_*` constants.
    pub const fn identifier(&self) -> u8 {
        self.identifier
    }

    /// Report bytes without the identifier byte.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

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
        // One pass: decode and send message by message. When a message fails after earlier
        // ones were sent, the accepted byte count is returned; the error is returned only
        // when no message was sent.
        let messages = Messages::new(buffer).map_err(|_| IoErrorKind::InvalidArgument)?;
        let mut sent = 0usize;
        for message in messages {
            let result = message
                .map_err(|_| IoErrorKind::InvalidArgument)
                .and_then(|message| {
                    let report = Report::from_message(&message);
                    self.0
                        .send_report(report.identifier(), report.bytes())
                        .map_err(map_error)
                });
            match result {
                Ok(()) => sent += 1,
                Err(error) => {
                    if sent == 0 {
                        return Err(error);
                    }
                    return Ok(sent * MESSAGE_SIZE);
                }
            }
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
        ControllerError::Busy => IoErrorKind::WouldBlock,
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
    use alloc::vec::Vec;
    use device::human_interface::INDICATOR_NUM_LOCK;
    use peripherals::human_interface::REPORT_DESCRIPTOR;

    /// Controller that records reports and can fail with a chosen error.
    struct RecordingController {
        reports: [Option<(u8, [u8; 8], usize)>; 8],
        count: usize,
        connected: bool,
        indicators: u8,
        fail_with: Option<ControllerError>,
        /// Fail only from the `fail_after`-th report on, to test partial writes.
        fail_after: Option<usize>,
    }

    impl RecordingController {
        fn new() -> Self {
            Self {
                reports: [None; 8],
                count: 0,
                connected: true,
                indicators: 0,
                fail_with: None,
                fail_after: None,
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
            if let Some(limit) = self.fail_after
                && self.count >= limit
            {
                return Err(ControllerError::Busy);
            }
            let mut bytes = [0u8; 8];
            bytes[..report.len()].copy_from_slice(report);
            self.reports[self.count % 8] = Some((identifier, bytes, report.len()));
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
                REPORT_IDENTIFIER_KEYBOARD,
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
            Some((
                REPORT_IDENTIFIER_CONSUMER,
                [0xCD, 0x00, 0, 0, 0, 0, 0, 0],
                2
            ))
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
            Some((
                REPORT_IDENTIFIER_MOUSE,
                [0x02, 0xFF, 0x02, 0x03, 0xFC, 0, 0, 0],
                5
            ))
        );
        assert_eq!(
            handle.0.reports[1],
            Some((REPORT_IDENTIFIER_SYSTEM, [0x82, 0, 0, 0, 0, 0, 0, 0], 1))
        );
    }

    #[test]
    fn a_failing_later_message_returns_the_accepted_byte_count() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        // The second report is refused with Busy.
        handle.0.fail_after = Some(1);
        let mut buffer = [0u8; 3 * MESSAGE_SIZE];
        buffer[..MESSAGE_SIZE].copy_from_slice(&Message::Consumer { usage: 0 }.encode());
        buffer[MESSAGE_SIZE..2 * MESSAGE_SIZE]
            .copy_from_slice(&Message::Consumer { usage: 1 }.encode());
        buffer[2 * MESSAGE_SIZE..].copy_from_slice(&Message::Consumer { usage: 2 }.encode());
        assert_eq!(handle.write(&buffer), Ok(MESSAGE_SIZE));
        assert_eq!(handle.0.count, 1);
    }

    #[test]
    fn an_invalid_first_message_fails_the_write_and_sends_nothing() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        // A length that is not a multiple of 8.
        assert_eq!(handle.write(&[0u8; 7]), Err(IoErrorKind::InvalidArgument));
        assert_eq!(handle.write(&[0u8; 9]), Err(IoErrorKind::InvalidArgument));
        // An unknown kind.
        let mut message = Message::System { usage: 0 }.encode();
        message[0] = 77;
        assert_eq!(handle.write(&message), Err(IoErrorKind::InvalidArgument));
        // A non-zero reserved byte.
        let mut message = Message::System { usage: 0 }.encode();
        message[2] = 1;
        assert_eq!(handle.write(&message), Err(IoErrorKind::InvalidArgument));
        assert_eq!(handle.0.count, 0);
    }

    #[test]
    fn an_invalid_later_message_returns_the_accepted_byte_count() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        let mut buffer = [0u8; 2 * MESSAGE_SIZE];
        buffer[..MESSAGE_SIZE].copy_from_slice(&Message::Consumer { usage: 0 }.encode());
        buffer[MESSAGE_SIZE] = 200;
        assert_eq!(handle.write(&buffer), Ok(MESSAGE_SIZE));
        assert_eq!(handle.0.count, 1);
    }

    #[test]
    fn controller_errors_map_to_file_system_errors() {
        let mut handle = HumanInterfaceHandle::new(RecordingController::new());
        for (error, expected) in [
            (ControllerError::NotConnected, IoErrorKind::NotConnected),
            (ControllerError::Busy, IoErrorKind::WouldBlock),
            (ControllerError::Other, IoErrorKind::InputOutput),
        ] {
            handle.0.fail_with = Some(error);
            assert_eq!(
                handle.write(&Message::System { usage: 0 }.encode()),
                Err(expected)
            );
        }
        assert_eq!(handle.0.count, 0);
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

    /// One parsed field of the descriptor: a report of `bits` bits on `report_id`.
    struct Field {
        report_id: u8,
        output: bool,
        bits: u32,
        /// Size of one element in bits.
        size: u32,
        /// Constant fields (padding) carry no usage.
        constant: bool,
        /// True when a Usage, or a Usage Minimum and Maximum, applies to the field.
        has_usage: bool,
        /// Array fields must carry their usage range as their logical range.
        array: bool,
        usage_minimum: u32,
        usage_maximum: u32,
        logical_minimum: i32,
        logical_maximum: i32,
    }

    /// Walk the descriptor item by item and return every Input and Output field.
    fn walk_descriptor(descriptor: &[u8]) -> Vec<Field> {
        let mut fields = Vec::new();
        let mut report_id = 0u8;
        let mut report_size = 0u32;
        let mut report_count = 0u32;
        let mut usage_minimum = 0u32;
        let mut usage_maximum = 0u32;
        let mut has_usage = false;
        let mut logical_minimum = 0i32;
        let mut logical_maximum = 0i32;
        let mut index = 0usize;
        while index < descriptor.len() {
            let prefix = descriptor[index];
            index += 1;
            if prefix == 0xFE {
                // Long item: the next byte is the data size, then the tag.
                let size = descriptor[index] as usize;
                index += 1 + size + 1;
                continue;
            }
            let size = match prefix & 0x03 {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            let mut data = 0u32;
            for shift in (0..size * 8).step_by(8) {
                data |= u32::from(descriptor[index]) << shift;
                index += 1;
            }
            let signed = |data: u32, size: usize| -> i32 {
                match size {
                    1 => data as u8 as i8 as i32,
                    2 => data as u16 as i16 as i32,
                    _ => data as i32,
                }
            };
            // Main items have type 00, global items type 01 and local items type 10 in
            // bits 2-3 of the prefix, so masking with 0xFC keeps the tag and type only.
            match prefix & 0xFC {
                0x04 => {}                                    // Usage Page (global)
                0x14 => logical_minimum = signed(data, size), // Logical Minimum (global)
                0x24 => logical_maximum = signed(data, size), // Logical Maximum (global)
                0x74 => report_size = data,                   // Report Size (global)
                0x84 => report_id = data as u8,               // Report ID (global)
                0x94 => report_count = data,                  // Report Count (global)
                0x08 => has_usage = true,                     // Usage (local)
                0x18 => {
                    // Usage Minimum (local)
                    usage_minimum = data;
                    has_usage = true;
                }
                0x28 => {
                    // Usage Maximum (local)
                    usage_maximum = data;
                    has_usage = true;
                }
                0x80 | 0x90 | 0xB0 => {
                    // Input, Output, Feature (main items).
                    let output = prefix & 0xFC == 0x90;
                    let flags = data as u8;
                    fields.push(Field {
                        report_id,
                        output,
                        bits: report_size * report_count,
                        size: report_size,
                        constant: flags & 0x01 != 0,
                        has_usage,
                        // Only Data, Array fields carry a usage range as their logical
                        // range; Constant and Variable fields do not.
                        array: flags & 0x03 == 0,
                        usage_minimum,
                        usage_maximum,
                        logical_minimum,
                        logical_maximum,
                    });
                    // Every Main item clears the local state, as the Linux parser does.
                    (usage_minimum, usage_maximum, has_usage) = (0, 0, false);
                }
                0xA0 | 0xC0 => {
                    // Collection, End Collection (main items).
                    (usage_minimum, usage_maximum, has_usage) = (0, 0, false);
                }
                tag if tag & 0x0C == 0x00 => {
                    panic!(
                        "reserved main item 0x{tag:02X} at offset {}",
                        index - 1 - size
                    );
                }
                tag if tag & 0x0C == 0x0C => {
                    // Type 11 is reserved: a wrong item size before it misaligns the walk.
                    panic!(
                        "reserved item type 0x{tag:02X} at offset {}",
                        index - 1 - size
                    );
                }
                _ => {}
            }
        }
        fields
    }

    #[test]
    fn descriptor_report_sizes_match_the_produced_reports() {
        let fields = walk_descriptor(REPORT_DESCRIPTOR);
        for message in [
            Message::Keyboard {
                modifiers: Modifiers::ALL,
                keys: [keyboard::F24; KEY_COUNT],
            },
            Message::Consumer {
                usage: consumer::VOLUME_UP,
            },
            Message::Mouse {
                buttons: MouseButtons::ALL,
                x: -128,
                y: 127,
                wheel: -1,
                pan: 1,
            },
            Message::System {
                usage: system::WAKE_UP,
            },
        ] {
            let report = Report::from_message(&message);
            let bits: u32 = fields
                .iter()
                .filter(|field| !field.output && field.report_id == report.identifier())
                .map(|field| field.bits)
                .sum();
            assert!(
                bits > 0,
                "the descriptor declares no input report for identifier {}",
                report.identifier()
            );
            assert_eq!(
                bits,
                report.bytes().len() as u32 * 8,
                "report size of identifier {} does not match the produced report",
                report.identifier()
            );
        }
    }

    #[test]
    fn descriptor_led_output_report_is_8_bits() {
        let fields = walk_descriptor(REPORT_DESCRIPTOR);
        let led_bits: u32 = fields
            .iter()
            .filter(|field| field.output && field.report_id == REPORT_IDENTIFIER_KEYBOARD)
            .map(|field| field.bits)
            .sum();
        assert_eq!(led_bits, 8);
    }

    #[test]
    fn descriptor_array_fields_carry_their_usage_range_as_logical_range() {
        let fields = walk_descriptor(REPORT_DESCRIPTOR);
        for field in fields.iter().filter(|field| field.array && !field.output) {
            assert_eq!(
                field.logical_minimum, field.usage_minimum as i32,
                "array field on report {} has a logical minimum different from its usage minimum",
                field.report_id
            );
            assert_eq!(
                field.logical_maximum, field.usage_maximum as i32,
                "array field on report {} has a logical maximum different from its usage maximum",
                field.report_id
            );
        }
    }

    #[test]
    fn descriptor_data_fields_have_a_usage() {
        let fields = walk_descriptor(REPORT_DESCRIPTOR);
        for field in fields.iter().filter(|field| !field.constant) {
            assert!(
                field.has_usage,
                "a data field on report {} has no usage",
                field.report_id
            );
        }
    }

    #[test]
    fn descriptor_mouse_axes_span_the_signed_byte_range() {
        let fields = walk_descriptor(REPORT_DESCRIPTOR);
        let axes: Vec<&Field> = fields
            .iter()
            .filter(|field| {
                !field.output && field.report_id == REPORT_IDENTIFIER_MOUSE && field.size == 8
            })
            .collect();
        assert!(!axes.is_empty());
        for field in axes {
            assert_eq!((field.logical_minimum, field.logical_maximum), (-128, 127));
        }
    }
}
