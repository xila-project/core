//! Bluetooth Low Energy controls for `/devices/bluetooth/hci0`.
//!
//! An opening owns its operations and event stream. A command returns when
//! accepted. Read its completion event before submitting the next command.
//! Each read returns one fixed-size [`Event`]. Use the VFS readiness API to
//! wait for events, or use a blocking read.

use file_system::{ControlCommand, IoErrorKind, define_command};

const KIND: u8 = crate::kind::BLUETOOTH;

define_command!(GET_CAPABILITIES, Read, KIND, 0, (), Capabilities);
define_command!(GET_STATE, Read, KIND, 1, (), State);
define_command!(START_SCAN, Write, KIND, 2, ScanConfiguration, ());
define_command!(STOP_SCAN, Write, KIND, 3, (), ());
define_command!(CONNECT, Write, KIND, 4, ConnectionRequest, ());
define_command!(DISCONNECT, Write, KIND, 5, ConnectionHandle, ());
define_command!(DISCOVER_SERVICES, Write, KIND, 6, ConnectionHandle, ());
define_command!(
    DISCOVER_CHARACTERISTICS,
    Write,
    KIND,
    7,
    AttributeRequest,
    ()
);
define_command!(DISCOVER_DESCRIPTORS, Write, KIND, 8, AttributeRequest, ());
define_command!(READ_ATTRIBUTE, Write, KIND, 9, AttributeRequest, ());
define_command!(WRITE_ATTRIBUTE, Write, KIND, 10, WriteRequest, ());
define_command!(SET_NOTIFICATIONS, Write, KIND, 11, NotificationRequest, ());
define_command!(START_ADVERTISING, Write, KIND, 12, AdvertisingData, ());
define_command!(STOP_ADVERTISING, Write, KIND, 13, (), ());

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Capabilities {
        /// Bit set of supported operations.
        pub features: u16,
        pub connection_limit: u8,
        pub value_capacity: u8,
    }
}

impl Capabilities {
    pub const CENTRAL: u16 = 1;
    pub const OBSERVER: u16 = 2;
    pub const BROADCASTER: u16 = 4;
    pub const GATT_CLIENT: u16 = 8;
}

file_system::plain_data! {
    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ConnectionHandle(pub u32);
}

impl ConnectionHandle {
    pub const NONE: Self = Self(0);
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct State {
        pub connection: ConnectionHandle,
        pub scanning: u8,
        pub advertising: u8,
        pub reserved: [u8; 2],
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Address {
        /// Address bytes in HCI order, least significant byte first.
        pub bytes: [u8; 6],
        pub kind: u8,
    }
}

impl Address {
    pub const PUBLIC: u8 = 0;
    pub const RANDOM: u8 = 1;
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ScanConfiguration {
        /// One for active scanning. Zero for passive scanning.
        pub active: u8,
        /// One to request duplicate filtering. Zero to receive duplicates.
        pub filter_duplicates: u8,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ConnectionRequest {
        /// Time limit in milliseconds. This must be greater than zero.
        pub timeout_milliseconds: u32,
        pub peer: Address,
        pub reserved: u8,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct AdvertisingData {
        pub length: u8,
        pub bytes: [u8; 31],
    }
}

impl AdvertisingData {
    pub fn as_slice(&self) -> Option<&[u8]> {
        self.bytes.get(..usize::from(self.length))
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ScanResult {
        pub peer: Address,
        pub rssi: i8,
        pub data: AdvertisingData,
        /// The HCI legacy advertising event type.
        pub event_type: u8,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Uuid {
        /// UUID bytes in Bluetooth order, least significant byte first.
        pub bytes: [u8; 16],
        /// Number of valid bytes: 2, 4 or 16.
        pub length: u8,
    }
}

impl Uuid {
    pub const fn short(value: u16) -> Self {
        let [low, high] = value.to_le_bytes();
        Self {
            bytes: [low, high, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            length: 2,
        }
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Service {
        pub first_handle: u16,
        pub last_handle: u16,
        pub uuid: Uuid,
        pub reserved: u8,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Characteristic {
        pub value_handle: u16,
        pub last_handle: u16,
        pub configuration_handle: u16,
        pub properties: u8,
        pub uuid: Uuid,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Descriptor {
        pub handle: u16,
        pub uuid: Uuid,
        pub reserved: u8,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct AttributeRequest {
        pub connection: ConnectionHandle,
        /// The service's first handle, characteristic value handle or attribute handle.
        pub handle: u16,
        pub reserved: u16,
    }
}

pub const VALUE_CAPACITY: usize = 41;

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct AttributeValue {
        pub handle: u16,
        pub length: u8,
        pub bytes: [u8; VALUE_CAPACITY],
    }
}

impl Default for AttributeValue {
    fn default() -> Self {
        Self {
            handle: 0,
            length: 0,
            bytes: [0; VALUE_CAPACITY],
        }
    }
}

impl AttributeValue {
    pub fn as_slice(&self) -> Option<&[u8]> {
        self.bytes.get(..usize::from(self.length))
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WriteRequest {
        pub connection: ConnectionHandle,
        pub value: AttributeValue,
    }
}

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct NotificationRequest {
        pub characteristic: AttributeRequest,
        /// One to subscribe to notifications. Zero to unsubscribe.
        pub enabled: u8,
        pub reserved: [u8; 3],
    }
}

const EVENT_PAYLOAD_SIZE: usize = size_of::<AttributeValue>();

file_system::plain_data! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Event {
        pub connection: ConnectionHandle,
        /// For a completion, the stable I/O error ID. Zero means success.
        pub status: u16,
        pub kind: u8,
        /// The command number for a completion or a discovery result.
        pub command: u8,
        pub length: u8,
        pub reserved: [u8; 3],
        payload: [u8; EVENT_PAYLOAD_SIZE],
    }
}

impl Default for Event {
    fn default() -> Self {
        Self::new(0, 0, ConnectionHandle::NONE)
    }
}

impl Event {
    /// No payload.
    pub const COMPLETION: u8 = 1;
    /// [`ScanResult`] payload.
    pub const SCAN_RESULT: u8 = 2;
    /// [`Service`] payload.
    pub const SERVICE: u8 = 3;
    /// [`Characteristic`] payload.
    pub const CHARACTERISTIC: u8 = 4;
    /// [`Descriptor`] payload.
    pub const DESCRIPTOR: u8 = 5;
    /// [`AttributeValue`] payload.
    pub const VALUE: u8 = 6;
    /// [`AttributeValue`] payload.
    pub const NOTIFICATION: u8 = 7;
    /// Disconnect reason, as an HCI error byte.
    pub const DISCONNECTED: u8 = 8;
    /// Saturating count of dropped events, as a `u32`.
    pub const OVERFLOW: u8 = 9;

    pub const fn new(kind: u8, command: u8, connection: ConnectionHandle) -> Self {
        Self {
            connection,
            status: 0,
            kind,
            command,
            length: 0,
            reserved: [0; 3],
            payload: [0; EVENT_PAYLOAD_SIZE],
        }
    }

    pub fn with_payload<T: file_system::PlainData>(mut self, mut payload: T) -> Self {
        const { assert!(size_of::<T>() <= EVENT_PAYLOAD_SIZE) };
        let bytes = shared::AnyByLayout::from_mutable(&mut payload).as_mutable_bytes();
        self.payload[..bytes.len()].copy_from_slice(bytes);
        self.length = bytes.len() as u8;
        self
    }

    /// Decode a payload after checking the event kind and length.
    pub fn payload<T: file_system::PlainData + Default>(&self, kind: u8) -> Option<T> {
        let bytes = self.payload.get(..usize::from(self.length))?;
        if self.kind != kind || bytes.len() != size_of::<T>() {
            return None;
        }
        let mut value = T::default();
        shared::AnyByLayout::from_mutable(&mut value)
            .as_mutable_bytes()
            .copy_from_slice(bytes);
        Some(value)
    }

    pub fn as_mutable_bytes(&mut self) -> &mut [u8] {
        shared::AnyByLayout::from_mutable(self).as_mutable_bytes()
    }

    pub fn completion(&self) -> Result<(), IoErrorKind> {
        match (self.kind, self.status) {
            (Self::COMPLETION, 0) => Ok(()),
            (Self::COMPLETION, status) => {
                Err(IoErrorKind::try_from(status).unwrap_or(IoErrorKind::InvalidData))
            }
            _ => Err(IoErrorKind::InvalidData),
        }
    }
}
