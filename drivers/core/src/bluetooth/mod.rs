//! Deterministic, in-memory Bluetooth Low Energy controller.
//! The simulator uses logical ticks. The test or demonstration advances
//! them explicitly. The controller never opens a host Bluetooth device.

mod attribute;

use core::{cell::RefCell, convert::Infallible};

use bt_hci::{
    PacketKind, ReadHciError,
    cmd::{
        Cmd,
        controller_baseband::{HostBufferSize, Reset, SetEventMask, SetEventMaskPage2},
        info::ReadBdAddr,
        le::{
            LeCreateConn, LeCreateConnCancel, LeReadBufferSize, LeReadFilterAcceptListSize,
            LeSetAdvData, LeSetAdvEnable, LeSetAdvParams, LeSetEventMask, LeSetRandomAddr,
            LeSetScanEnable, LeSetScanParams,
        },
        link_control::Disconnect,
    },
    controller::ExternalController,
};
use bt_hci_transport::{PacketToController, PacketToHost, Transport};
use device::bluetooth::{Address, AdvertisingData};
use embedded_io::{ErrorKind, ErrorType};
use heapless::Vec;
use synchronization::{blocking_mutex::raw::NoopRawMutex, signal::Signal};

pub use attribute::{Attribute, AttributeData};

const CONNECTION_HANDLE: u16 = 1;
const ATT_CHANNEL: u16 = 4;
const MAXIMUM_TRANSMISSION_UNIT: usize = 44;
const MAXIMUM_PACKET_SIZE: usize = MAXIMUM_TRANSMISSION_UNIT + 8;
const PACKET_LIMIT: usize = 32;
const PEER_LIMIT: usize = 8;

const RESET: u16 = Reset::OPCODE.to_raw();
const READ_ADDRESS: u16 = ReadBdAddr::OPCODE.to_raw();
const READ_BUFFER_SIZE: u16 = LeReadBufferSize::OPCODE.to_raw();
const READ_ACCEPT_LIST_SIZE: u16 = LeReadFilterAcceptListSize::OPCODE.to_raw();
const SET_SCAN_ENABLE: u16 = LeSetScanEnable::OPCODE.to_raw();
const CREATE_CONNECTION: u16 = LeCreateConn::OPCODE.to_raw();
const CANCEL_CONNECTION: u16 = LeCreateConnCancel::OPCODE.to_raw();
const DISCONNECT: u16 = Disconnect::OPCODE.to_raw();
const SET_ADVERTISING: u16 = LeSetAdvEnable::OPCODE.to_raw();

type Result<T> = core::result::Result<T, TransportError>;

#[derive(Clone, Copy, Debug)]
pub struct TransportError(ErrorKind);

impl core::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Simulated HCI transport: {:?}", self.0)
    }
}

impl core::error::Error for TransportError {}

impl embedded_io::Error for TransportError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

impl From<ReadHciError<Infallible>> for TransportError {
    fn from(error: ReadHciError<Infallible>) -> Self {
        Self(embedded_io::Error::kind(&error))
    }
}

/// One peer and its attribute table. Use strictly increasing handles.
/// Advertisement and connection delays are in logical ticks.
pub struct Peripheral<'database> {
    pub address: Address,
    pub advertising: AdvertisingData,
    pub rssi: i8,
    /// Delay after scan starts. None disables advertising.
    pub advertisement_delay: Option<u64>,
    /// Delay after a connection request. None makes requests time out.
    pub connection_delay: Option<u64>,
    /// Delay before ATT responses and notifications.
    pub attribute_delay: u64,
    pub attributes: &'database mut [Attribute],
}

/// Simulated controller state shared by the test and the HCI transport.
///
/// Run the controller and the code that drives the simulation on one executor.
/// The simulation uses `RefCell` and `NoopRawMutex` for single-threaded access.
/// The controller borrows this state and cannot be sent to another thread.
pub struct Simulation<'database> {
    state: RefCell<State<'database>>,
    changed: Signal<NoopRawMutex, ()>,
}

impl<'database> Simulation<'database> {
    pub fn new(
        address: [u8; 6],
        peripherals: &'database mut [Peripheral<'database>],
    ) -> Result<Self> {
        if peripherals.len() > PEER_LIMIT
            || peripherals.iter().any(|peer| {
                peer.address.kind > Address::RANDOM
                    || peer.advertising.as_slice().is_none()
                    || !attribute::valid(peer.attributes)
            })
        {
            return Err(TransportError(ErrorKind::InvalidInput));
        }
        Ok(Self {
            state: RefCell::new(State {
                address,
                peripherals,
                pending: Vec::new(),
                tick: 0,
                connected: None,
                subscribed: Vec::new(),
                advertising: false,
                scanning: false,
            }),
            changed: Signal::new(),
        })
    }

    /// The standard HCI adapter implements the controller and typed command
    /// traits over the simulator's packet transport.
    pub fn controller(&self) -> ExternalController<SimulatedTransport<'_, 'database>, 1> {
        ExternalController::new(SimulatedTransport(self))
    }

    pub fn advance(&self, ticks: u64) {
        let mut state = self.state.borrow_mut();
        state.tick = state.tick.saturating_add(ticks);
        drop(state);
        self.changed.signal(());
    }

    pub fn is_advertising(&self) -> bool {
        self.state.borrow().advertising
    }

    /// Queue an additional report from an existing peer while scanning.
    pub fn advertise_after(&self, peer: Address, delay: u64) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if !state.scanning {
            return Err(TransportError(ErrorKind::InvalidInput));
        }
        let index = state
            .peripherals
            .iter()
            .position(|peripheral| peripheral.address == peer)
            .ok_or(TransportError(ErrorKind::NotFound))?;
        state.schedule(delay, Effect::Report(index))?;
        drop(state);
        self.changed.signal(());
        Ok(())
    }

    /// Queue an unsolicited disconnection after a number of ticks.
    pub fn disconnect_after(&self, delay: u64, reason: u8) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if state.connected.is_none() {
            return Err(TransportError(ErrorKind::NotConnected));
        }
        state.schedule(delay, Effect::Disconnect(reason))?;
        drop(state);
        self.changed.signal(());
        Ok(())
    }
}

/// Packet transport used by [`Simulation::controller`].
pub struct SimulatedTransport<'simulation, 'database>(&'simulation Simulation<'database>);

impl ErrorType for SimulatedTransport<'_, '_> {
    type Error = TransportError;
}

impl Transport for SimulatedTransport<'_, '_> {
    async fn read<'a, P: PacketToHost<'a>>(&self, buffer: &'a mut [u8]) -> Result<P> {
        let packet = loop {
            let packet = self.0.state.borrow_mut().take_ready();
            if let Some(packet) = packet {
                break packet;
            }
            self.0.changed.wait().await;
        };
        P::read_hci(packet.kind, &mut packet.bytes.as_slice(), buffer).map_err(Into::into)
    }

    async fn write<P: PacketToController>(&self, packet: &P) -> Result<()> {
        let mut bytes = [0; MAXIMUM_PACKET_SIZE];
        let frame = bytes
            .get_mut(..packet.size())
            .ok_or(TransportError(ErrorKind::InvalidInput))?;
        packet
            .write_hci(&mut *frame)
            .map_err(|_| TransportError(ErrorKind::InvalidInput))?;
        {
            let mut state = self.0.state.borrow_mut();
            match P::KIND {
                PacketKind::Cmd => state.command(frame)?,
                PacketKind::AclData => state.attribute(frame)?,
                _ => return Err(TransportError(ErrorKind::Unsupported)),
            }
        }
        self.0.changed.signal(());
        Ok(())
    }
}

#[derive(Clone)]
struct Packet {
    kind: PacketKind,
    bytes: Vec<u8, MAXIMUM_PACKET_SIZE>,
}

impl Packet {
    fn event(code: u8, parameters: &[u8]) -> Self {
        let mut bytes = Vec::new();
        bytes
            .extend_from_slice(&[code, parameters.len() as u8])
            .unwrap();
        bytes.extend_from_slice(parameters).unwrap();
        Self {
            kind: PacketKind::Event,
            bytes,
        }
    }

    fn attribute(parameters: &[u8]) -> Self {
        let mut bytes = Vec::new();
        bytes
            .extend_from_slice(&(CONNECTION_HANDLE | 0x2000).to_le_bytes())
            .unwrap();
        bytes
            .extend_from_slice(&((parameters.len() + 4) as u16).to_le_bytes())
            .unwrap();
        bytes
            .extend_from_slice(&(parameters.len() as u16).to_le_bytes())
            .unwrap();
        bytes.extend_from_slice(&ATT_CHANNEL.to_le_bytes()).unwrap();
        bytes.extend_from_slice(parameters).unwrap();
        Self {
            kind: PacketKind::AclData,
            bytes,
        }
    }
}

enum Effect {
    Packet(Packet),
    Report(usize),
    Connect(usize),
    Attribute(Packet),
    Disconnect(u8),
}

struct Scheduled {
    tick: u64,
    effect: Effect,
}

struct State<'database> {
    address: [u8; 6],
    peripherals: &'database mut [Peripheral<'database>],
    pending: Vec<Scheduled, PACKET_LIMIT>,
    tick: u64,
    connected: Option<usize>,
    subscribed: Vec<u16, 8>,
    advertising: bool,
    scanning: bool,
}

impl State<'_> {
    fn schedule(&mut self, delay: u64, effect: Effect) -> Result<()> {
        self.pending
            .push(Scheduled {
                tick: self.tick.saturating_add(delay),
                effect,
            })
            .map_err(|_| TransportError(ErrorKind::OutOfMemory))
    }

    fn take_ready(&mut self) -> Option<Packet> {
        let index = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.tick <= self.tick)
            .min_by_key(|(_, frame)| frame.tick)
            .map(|(index, _)| index)?;
        Some(match self.pending.remove(index).effect {
            Effect::Packet(packet) | Effect::Attribute(packet) => packet,
            Effect::Report(index) => {
                let peripheral = &self.peripherals[index];
                let data = peripheral.advertising.as_slice().unwrap();
                let mut parameters = [0; 43];
                // Subevent, report count, event type, address type.
                parameters[..4].copy_from_slice(&[2, 1, 0, peripheral.address.kind]);
                parameters[4..10].copy_from_slice(&peripheral.address.bytes);
                parameters[10] = data.len() as u8;
                parameters[11..11 + data.len()].copy_from_slice(data);
                parameters[11 + data.len()] = peripheral.rssi as u8;
                Packet::event(0x3e, &parameters[..12 + data.len()])
            }
            Effect::Connect(index) => {
                self.connected = Some(index);
                let peripheral = &self.peripherals[index];
                connection_complete(0, peripheral.address)
            }
            Effect::Disconnect(reason) => {
                self.connected = None;
                self.subscribed.clear();
                self.pending.retain(|entry| {
                    !matches!(entry.effect, Effect::Attribute(_) | Effect::Disconnect(_))
                });
                let [low, high] = CONNECTION_HANDLE.to_le_bytes();
                Packet::event(0x05, &[0, low, high, reason])
            }
        })
    }

    fn complete(&mut self, opcode: u16, status: u8, data: &[u8]) -> Result<()> {
        let [low, high] = opcode.to_le_bytes();
        let mut parameters = [0; 10];
        parameters[..4].copy_from_slice(&[1, low, high, status]);
        parameters[4..4 + data.len()].copy_from_slice(data);
        self.schedule(
            0,
            Effect::Packet(Packet::event(0x0e, &parameters[..4 + data.len()])),
        )
    }

    fn command(&mut self, bytes: &[u8]) -> Result<()> {
        let [low, high, _, parameters @ ..] = bytes else {
            return Err(TransportError(ErrorKind::InvalidInput));
        };
        let opcode = u16::from_le_bytes([*low, *high]);
        match opcode {
            RESET => {
                self.pending.clear();
                self.connected = None;
                self.advertising = false;
                self.scanning = false;
                self.subscribed.clear();
            }
            READ_ADDRESS => {
                let address = self.address;
                return self.complete(opcode, 0, &address);
            }
            READ_BUFFER_SIZE => return self.complete(opcode, 0, &[48, 0, 4]),
            READ_ACCEPT_LIST_SIZE => return self.complete(opcode, 0, &[PEER_LIMIT as u8]),
            SET_SCAN_ENABLE => {
                self.pending
                    .retain(|entry| !matches!(entry.effect, Effect::Report(_)));
                self.complete(opcode, 0, &[])?;
                self.scanning = parameters[0] == 1;
                if self.scanning {
                    for index in 0..self.peripherals.len() {
                        if let Some(delay) = self.peripherals[index].advertisement_delay {
                            self.schedule(delay, Effect::Report(index))?;
                        }
                    }
                }
                return Ok(());
            }
            CREATE_CONNECTION => {
                self.schedule(0, Effect::Packet(Packet::event(0x0f, &[0, 1, *low, *high])))?;
                let index = self.peripherals.iter().position(|peer| {
                    peer.address.kind == parameters[5] && peer.address.bytes == parameters[6..12]
                });
                if let Some(index) = index
                    && let Some(delay) = self.peripherals[index].connection_delay
                {
                    self.schedule(delay, Effect::Connect(index))?;
                }
                return Ok(());
            }
            CANCEL_CONNECTION => {
                self.pending
                    .retain(|entry| !matches!(entry.effect, Effect::Connect(_)));
                self.complete(opcode, 0, &[])?;
                return self.schedule(
                    0,
                    Effect::Packet(connection_complete(2, Address::default())),
                );
            }
            DISCONNECT => {
                if self.connected.is_none() {
                    return self.complete(opcode, 2, &[]);
                }
                self.complete(opcode, 0, &[])?;
                return self.schedule(0, Effect::Disconnect(parameters[2]));
            }
            SET_ADVERTISING => self.advertising = parameters[0] != 0,
            command
                if [
                    SetEventMask::OPCODE.to_raw(),
                    SetEventMaskPage2::OPCODE.to_raw(),
                    LeSetEventMask::OPCODE.to_raw(),
                    HostBufferSize::OPCODE.to_raw(),
                    LeSetRandomAddr::OPCODE.to_raw(),
                    LeSetScanParams::OPCODE.to_raw(),
                    LeSetAdvParams::OPCODE.to_raw(),
                    LeSetAdvData::OPCODE.to_raw(),
                ]
                .contains(&command) => {}
            _ => return self.complete(opcode, 1, &[]),
        }
        self.complete(opcode, 0, &[])
    }

    fn attribute(&mut self, bytes: &[u8]) -> Result<()> {
        let Some(index) = self.connected else {
            return Ok(());
        };
        let [_, _, _, _, _, _, low, high, request @ ..] = bytes else {
            return Err(TransportError(ErrorKind::InvalidInput));
        };
        let [handle_low, handle_high] = CONNECTION_HANDLE.to_le_bytes();
        self.schedule(
            0,
            Effect::Packet(Packet::event(0x13, &[1, handle_low, handle_high, 1, 0])),
        )?;
        if u16::from_le_bytes([*low, *high]) != ATT_CHANNEL {
            return Ok(());
        }

        let peer = &mut self.peripherals[index];
        let delay = peer.attribute_delay;
        let (response, notification) =
            attribute::respond(peer.attributes, &mut self.subscribed, request);
        self.schedule(delay, Effect::Attribute(Packet::attribute(&response)))?;
        if let Some(notification) = notification {
            self.schedule(delay, Effect::Attribute(Packet::attribute(&notification)))?;
        }
        Ok(())
    }
}

fn connection_complete(status: u8, address: Address) -> Packet {
    let [low, high] = CONNECTION_HANDLE.to_le_bytes();
    let mut parameters = [0; 19];
    parameters[..6].copy_from_slice(&[1, status, low, high, 0, address.kind]);
    parameters[6..12].copy_from_slice(&address.bytes);
    parameters[12..19].copy_from_slice(&[24, 0, 0, 0, 200, 0, 0]);
    Packet::event(0x3e, &parameters)
}
