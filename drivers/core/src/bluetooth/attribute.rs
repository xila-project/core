use device::bluetooth::{AttributeValue, Uuid};
use heapless::Vec;

use super::MAXIMUM_TRANSMISSION_UNIT;

const PRIMARY_SERVICE: u16 = 0x2800;
const CHARACTERISTIC_DECLARATION: u16 = 0x2803;
const CLIENT_CHARACTERISTIC_CONFIGURATION: u16 = 0x2902;
const READ: u8 = 0x02;
const WRITE: u8 = 0x08;
const NOTIFY: u8 = 0x10;

const INVALID_HANDLE: u8 = 0x01;
const READ_NOT_PERMITTED: u8 = 0x02;
const WRITE_NOT_PERMITTED: u8 = 0x03;
const INVALID_PDU: u8 = 0x04;
const REQUEST_NOT_SUPPORTED: u8 = 0x06;
const INVALID_OFFSET: u8 = 0x07;
const ATTRIBUTE_NOT_FOUND: u8 = 0x0a;
const INVALID_VALUE_LENGTH: u8 = 0x0d;
const INSUFFICIENT_RESOURCES: u8 = 0x11;

type Payload = Vec<u8, MAXIMUM_TRANSMISSION_UNIT>;

/// One attribute in a simulated peripheral's database.
pub struct Attribute {
    pub handle: u16,
    pub data: AttributeData,
}

/// Attribute data. UUIDs in the simulated on-wire database may be 16 or
/// 128 bits. A subscription applies to the declared characteristic value.
pub enum AttributeData {
    Service {
        uuid: Uuid,
        last_handle: u16,
    },
    Characteristic {
        uuid: Uuid,
        properties: u8,
        value_handle: u16,
    },
    Value {
        uuid: Uuid,
        properties: u8,
        value: AttributeValue,
    },
    Descriptor {
        uuid: Uuid,
        value: AttributeValue,
    },
    ClientConfiguration {
        value_handle: u16,
    },
}

impl AttributeData {
    fn attribute_type(&self) -> Uuid {
        match self {
            Self::Service { .. } => Uuid::short(PRIMARY_SERVICE),
            Self::Characteristic { .. } => Uuid::short(CHARACTERISTIC_DECLARATION),
            Self::Value { uuid, .. } | Self::Descriptor { uuid, .. } => *uuid,
            Self::ClientConfiguration { .. } => Uuid::short(CLIENT_CHARACTERISTIC_CONFIGURATION),
        }
    }

    fn schema_uuid(&self) -> Uuid {
        match self {
            Self::Service { uuid, .. } | Self::Characteristic { uuid, .. } => *uuid,
            _ => self.attribute_type(),
        }
    }
}

pub(super) fn valid(attributes: &[Attribute]) -> bool {
    attributes
        .windows(2)
        .all(|pair| pair[0].handle < pair[1].handle)
        && attributes.iter().all(|attribute| {
            attribute.handle != 0
                && matches!(attribute.data.schema_uuid().length, 2 | 16)
                && match &attribute.data {
                    AttributeData::Service { last_handle, .. } => *last_handle >= attribute.handle,
                    AttributeData::Characteristic { value_handle, .. }
                    | AttributeData::ClientConfiguration { value_handle } => {
                        attributes.iter().any(|entry| {
                            entry.handle == *value_handle
                                && matches!(entry.data, AttributeData::Value { .. })
                        })
                    }
                    AttributeData::Value { value, .. }
                    | AttributeData::Descriptor { value, .. } => value.as_slice().is_some(),
                }
        })
}

pub(super) fn respond(
    attributes: &mut [Attribute],
    subscriptions: &mut Vec<u16, 8>,
    request: &[u8],
) -> (Payload, Option<Payload>) {
    let opcode = request.first().copied().unwrap_or_default();
    let handle = request.get(1..3).map_or(0, word);
    let result = match request {
        [0x02, _, _] => Ok((payload(&[0x03, MAXIMUM_TRANSMISSION_UNIT as u8, 0]), None)),
        [0x04, _, _, end_low, end_high] => discover(
            attributes,
            handle,
            u16::from_le_bytes([*end_low, *end_high]),
            Discovery::Information,
        )
        .map(|packet| (packet, None)),
        [0x08, _, _, end_low, end_high, 0x03, 0x28] => discover(
            attributes,
            handle,
            u16::from_le_bytes([*end_low, *end_high]),
            Discovery::Characteristics,
        )
        .map(|packet| (packet, None)),
        [0x10, _, _, end_low, end_high, 0x00, 0x28] => discover(
            attributes,
            handle,
            u16::from_le_bytes([*end_low, *end_high]),
            Discovery::Services,
        )
        .map(|packet| (packet, None)),
        [0x0a, _, _] => {
            read(attributes, subscriptions, handle, 0, 0x0b).map(|packet| (packet, None))
        }
        [0x0c, _, _, offset_low, offset_high] => read(
            attributes,
            subscriptions,
            handle,
            u16::from_le_bytes([*offset_low, *offset_high]),
            0x0d,
        )
        .map(|packet| (packet, None)),
        [0x12, _, _, data @ ..] => write(attributes, subscriptions, handle, data)
            .map(|notification| (payload(&[0x13]), notification)),
        [0x04 | 0x08 | 0x0a | 0x0c | 0x10 | 0x12, ..] => Err(INVALID_PDU),
        _ => Err(REQUEST_NOT_SUPPORTED),
    };

    result.unwrap_or_else(|error| {
        let [low, high] = handle.to_le_bytes();
        (payload(&[0x01, opcode, low, high, error]), None)
    })
}

#[derive(Clone, Copy)]
enum Discovery {
    Services,
    Characteristics,
    Information,
}

fn discover(
    attributes: &[Attribute],
    start: u16,
    end: u16,
    kind: Discovery,
) -> Result<Payload, u8> {
    if start == 0 || start > end {
        return Err(INVALID_HANDLE);
    }
    let mut response = payload(&[
        match kind {
            Discovery::Services => 0x11,
            Discovery::Characteristics => 0x09,
            Discovery::Information => 0x05,
        },
        0,
    ]);
    let mut width = None;
    for entry in attributes
        .iter()
        .filter(|entry| (start..=end).contains(&entry.handle))
    {
        let mut item = payload(&entry.handle.to_le_bytes());
        let uuid = match (kind, &entry.data) {
            (Discovery::Services, AttributeData::Service { uuid, last_handle }) => {
                item.extend_from_slice(&last_handle.to_le_bytes()).unwrap();
                *uuid
            }
            (
                Discovery::Characteristics,
                AttributeData::Characteristic {
                    uuid,
                    properties,
                    value_handle,
                },
            ) => {
                item.push(*properties).unwrap();
                item.extend_from_slice(&value_handle.to_le_bytes()).unwrap();
                *uuid
            }
            (Discovery::Information, entry) => entry.attribute_type(),
            _ => continue,
        };
        item.extend_from_slice(&uuid.bytes[..usize::from(uuid.length)])
            .unwrap();
        if width.is_some_and(|width| item.len() != width)
            || response.extend_from_slice(&item).is_err()
        {
            break;
        }
        width = Some(item.len());
        response[1] = match kind {
            Discovery::Information => {
                if uuid.length == 2 {
                    1
                } else {
                    2
                }
            }
            _ => item.len() as u8,
        };
    }
    width.map(|_| response).ok_or(ATTRIBUTE_NOT_FOUND)
}

fn read(
    attributes: &[Attribute],
    subscriptions: &[u16],
    handle: u16,
    offset: u16,
    response_code: u8,
) -> Result<Payload, u8> {
    let entry = attributes
        .iter()
        .find(|entry| entry.handle == handle)
        .ok_or(INVALID_HANDLE)?;
    let mut response = payload(&[response_code]);
    let mut metadata = Payload::new();
    let value = match &entry.data {
        AttributeData::Value {
            properties, value, ..
        } if properties & READ != 0 => value.as_slice().unwrap(),
        AttributeData::Descriptor { value, .. } => value.as_slice().unwrap(),
        AttributeData::ClientConfiguration { value_handle } => {
            metadata
                .extend_from_slice(&[u8::from(subscriptions.contains(value_handle)), 0])
                .unwrap();
            &metadata
        }
        AttributeData::Service { uuid, .. } => &uuid.bytes[..usize::from(uuid.length)],
        AttributeData::Characteristic {
            uuid,
            properties,
            value_handle,
        } => {
            metadata.push(*properties).unwrap();
            metadata
                .extend_from_slice(&value_handle.to_le_bytes())
                .unwrap();
            metadata
                .extend_from_slice(&uuid.bytes[..usize::from(uuid.length)])
                .unwrap();
            &metadata
        }
        _ => return Err(READ_NOT_PERMITTED),
    };
    let value = value.get(usize::from(offset)..).ok_or(INVALID_OFFSET)?;
    response.extend_from_slice(value).unwrap();
    Ok(response)
}

fn write(
    attributes: &mut [Attribute],
    subscriptions: &mut Vec<u16, 8>,
    handle: u16,
    data: &[u8],
) -> Result<Option<Payload>, u8> {
    let entry = attributes
        .iter_mut()
        .find(|entry| entry.handle == handle)
        .ok_or(INVALID_HANDLE)?;
    let (value, notify) = match &mut entry.data {
        AttributeData::Value {
            properties, value, ..
        } if *properties & WRITE != 0 => (
            value,
            *properties & NOTIFY != 0 && subscriptions.contains(&handle),
        ),
        AttributeData::Descriptor { value, .. } => (value, false),
        AttributeData::ClientConfiguration { value_handle } => {
            let existing = subscriptions.iter().position(|item| item == value_handle);
            match (data, existing) {
                ([0, 0], Some(index)) => {
                    subscriptions.swap_remove(index);
                }
                ([1, 0], None) => subscriptions
                    .push(*value_handle)
                    .map_err(|_| INSUFFICIENT_RESOURCES)?,
                ([0 | 1, 0], _) => {}
                _ => return Err(INVALID_VALUE_LENGTH),
            }
            return Ok(None);
        }
        _ => return Err(WRITE_NOT_PERMITTED),
    };
    if data.len() > value.bytes.len() {
        return Err(INVALID_VALUE_LENGTH);
    }
    value.length = data.len() as u8;
    value.bytes[..data.len()].copy_from_slice(data);

    Ok(notify.then(|| {
        let [low, high] = handle.to_le_bytes();
        let mut notification = payload(&[0x1b, low, high]);
        notification.extend_from_slice(data).unwrap();
        notification
    }))
}

fn word(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn payload(bytes: &[u8]) -> Payload {
    Payload::from_slice(bytes).unwrap()
}
