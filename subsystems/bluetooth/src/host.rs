use core::{
    cell::{Cell, RefCell},
    future::pending,
};

use bt_hci::{
    cmd::le::{LeSetAdvEnable, LeSetScanParams},
    controller::ControllerCmdSync,
    param::{AddrKind, BdAddr, FilterDuplicates, LeAdvReportsIter},
};
use embassy_futures::select::{Either, Either3, select, select3};
use embassy_time::{Duration, with_timeout};
use file_system::{ControlCommand, IoErrorKind, Result};
use heapless::Vec;
use trouble_host::{
    BleHostError, Error,
    prelude::{
        Advertisement, AdvertisementParameters, AsGatt, ConnectConfig, Connection, ConnectionEvent,
        DefaultPacketPool, EventHandler, GattClient, HostResources, ScanConfig, Scanner,
        ServiceHandle, Stack,
    },
};

use crate::{node::Request, *};

const DISCOVERY_LIMIT: usize = 8;
type Pool = DefaultPacketPool;
type Host<'a, C> = Stack<'a, C, Pool>;
type Client<'a, C> = GattClient<'a, C, Pool, DISCOVERY_LIMIT>;
type HostCharacteristic = trouble_host::prelude::Characteristic<[u8]>;

/// Controller requirements for the platform that starts the host runner.
/// Use the typed commands and transport in [`crate::hci`] to implement them.
pub trait Controller: trouble_host::Controller + ControllerCmdSync<LeSetScanParams> {}

impl<C> Controller for C where C: trouble_host::Controller + ControllerCmdSync<LeSetScanParams> {}

impl BluetoothDevice {
    /// Run the service on a local Xila task. The platform supplies the HCI
    /// controller, critical section implementation and Embassy time driver.
    /// Cancelling the future stops the service. The platform must reset or
    /// discard the controller before restarting after a failure.
    pub async fn run<C: Controller>(&self, controller: C) -> Result<()> {
        let _running = self.begin_run()?;
        let mut resources = HostResources::<Pool, 1, 1>::new();
        let stack = trouble_host::new(controller, &mut resources).build();
        let mut runner = stack.runner();
        match select(runner.run_with_handler(&Reports(self)), serve(self, &stack)).await {
            Either::First(result) => result.map_err(map_error),
            Either::Second(()) => Ok(()),
        }
    }
}

struct Reports<'a>(&'a BluetoothDevice);

impl EventHandler for Reports<'_> {
    fn on_adv_reports(&self, reports: LeAdvReportsIter<'_>) {
        for report in reports.flatten() {
            let mut result = ScanResult {
                peer: Address {
                    bytes: report.addr.raw().try_into().expect("six address bytes"),
                    kind: report.addr_kind.as_raw(),
                },
                rssi: report.rssi,
                event_type: report.event_kind as u8,
                ..ScanResult::default()
            };
            if report.data.len() > result.data.bytes.len() {
                continue;
            }
            result.data.length = report.data.len() as u8;
            result.data.bytes[..report.data.len()].copy_from_slice(report.data);
            self.0.emit(
                Event::new(Event::SCAN_RESULT, 0, ConnectionHandle::NONE).with_payload(result),
            );
        }
    }
}

async fn serve<C: Controller>(device: &BluetoothDevice, stack: &Host<'_, C>) {
    loop {
        device.wait_open().await;
        let mut scanning = None;
        let mut advertising = None;
        let mut link = None;
        let session = async {
            let mut central = stack.central();
            let mut peripheral = stack.peripheral();
            loop {
                let (command, request) = device.next_request().await;
                let result = match request {
                    Request::StartScan(configuration)
                        if scanning.is_none() && advertising.is_none() =>
                    {
                        let mut scanner = Scanner::new(&mut central);
                        let configuration = ScanConfig {
                            active: configuration.active != 0,
                            filter_duplicates: if configuration.filter_duplicates != 0 {
                                FilterDuplicates::Enabled
                            } else {
                                FilterDuplicates::Disabled
                            },
                            ..Default::default()
                        };
                        device.set_scanning(true);
                        match scanner.scan(&configuration).await {
                            Ok(scan) => {
                                scanning = Some(scan);
                                Ok(())
                            }
                            Err(error) => {
                                device.set_scanning(false);
                                Err(map_error(error))
                            }
                        }
                    }
                    Request::StopScan => {
                        device.set_scanning(false);
                        if let Some(scan) = scanning.take() {
                            scan.stop().await;
                        }
                        Ok(())
                    }
                    Request::StartAdvertising(data)
                        if advertising.is_none() && scanning.is_none() =>
                    {
                        let advertisement = Advertisement::NonconnectableNonscannableUndirected {
                            adv_data: data.as_slice().expect("validated advertising length"),
                        };
                        match peripheral
                            .advertise(&AdvertisementParameters::default(), advertisement)
                            .await
                        {
                            Ok(advertiser) => {
                                advertising = Some(advertiser);
                                device.set_advertising(true);
                                Ok(())
                            }
                            Err(error) => Err(map_error(error)),
                        }
                    }
                    Request::StopAdvertising => {
                        if advertising.is_some()
                            && let Err(error) = stack.command(LeSetAdvEnable::new(false)).await
                        {
                            Err(map_error(error))
                        } else {
                            drop(advertising.take());
                            device.set_advertising(false);
                            Ok(())
                        }
                    }
                    Request::Connect(request) if scanning.is_none() && advertising.is_none() => {
                        let kind = if request.peer.kind == Address::PUBLIC {
                            AddrKind::PUBLIC
                        } else {
                            AddrKind::RANDOM
                        };
                        let peer =
                            trouble_host::Address::new(kind, BdAddr::new(request.peer.bytes));
                        let configuration = ConnectConfig {
                            scan_config: ScanConfig {
                                filter_accept_list: &[peer],
                                ..Default::default()
                            },
                            connect_params: Default::default(),
                        };
                        let result = with_timeout(
                            Duration::from_millis(u64::from(request.timeout_milliseconds)),
                            async {
                                link =
                                    Some(central.connect(&configuration).await.map_err(map_error)?);
                                Client::new(stack, link.as_ref().expect("connected link"))
                                    .await
                                    .map_err(map_error)
                            },
                        )
                        .await;
                        let result = match result {
                            Ok(Ok(client)) => {
                                let handle = device.connected();
                                device.complete(command, handle, Ok(()));
                                serve_connection(
                                    device,
                                    handle,
                                    link.as_ref().expect("connected link"),
                                    &client,
                                )
                                .await;
                                drop(link.take());
                                continue;
                            }
                            Ok(Err(error)) => Err(error),
                            Err(_) => Err(IoErrorKind::TimedOut),
                        };
                        device.complete(command, ConnectionHandle::NONE, result);
                        finish_link(link.take()).await;
                        continue;
                    }
                    Request::Connect(_) | Request::StartScan(_) | Request::StartAdvertising(_) => {
                        Err(IoErrorKind::ResourceBusy)
                    }
                    _ => Err(IoErrorKind::NotConnected),
                };
                device.complete(command, ConnectionHandle::NONE, result);
            }
        };
        select(device.wait_closed(), session).await;
        device.set_scanning(false);
        if let Some(scan) = scanning {
            scan.stop().await;
        }
        if advertising.is_some() {
            let _ = stack.command(LeSetAdvEnable::new(false)).await;
        }
        drop(advertising);
        finish_link(link).await;
        device.finish_close();
    }
}

async fn serve_connection<C: Controller>(
    device: &BluetoothDevice,
    handle: ConnectionHandle,
    connection: &Connection<'_, Pool>,
    client: &Client<'_, C>,
) {
    let mut listener = match client.listen_all() {
        Ok(listener) => listener,
        Err(_) => {
            connection.disconnect();
            let reason = disconnected(connection).await;
            device.disconnected();
            emit(device, handle, 0, Event::DISCONNECTED, reason);
            return;
        }
    };
    let active = Cell::new(None);
    let services = RefCell::new(Vec::<ServiceHandle, DISCOVERY_LIMIT>::new());
    let subscriptions = RefCell::new(Vec::<u16, DISCOVERY_LIMIT>::new());

    let notifications = async {
        loop {
            let notification = listener.next().await;
            if notification.is_indication() {
                let _ = client.confirm_indication().await;
                continue;
            }
            if subscriptions.borrow().contains(&notification.handle()) {
                let mut value = AttributeValue {
                    handle: notification.handle(),
                    ..AttributeValue::default()
                };
                let bytes = notification.as_ref();
                if bytes.len() <= value.bytes.len() {
                    value.length = bytes.len() as u8;
                    value.bytes[..bytes.len()].copy_from_slice(bytes);
                    emit(device, handle, 0, Event::NOTIFICATION, value);
                }
            }
        }
    };

    let commands = async {
        loop {
            let (command, request) = device.next_request().await;
            active.set(Some(command));
            if matches!(request, Request::Disconnect) {
                connection.disconnect();
                pending::<()>().await;
            }
            let result = run_gatt(
                device,
                handle,
                client,
                &services,
                &subscriptions,
                command,
                request,
                connection.att_mtu(),
            )
            .await;
            device.complete(command, handle, result);
            active.set(None);
        }
    };

    let reason = match select3(
        disconnected(connection),
        client.task(),
        select(notifications, commands),
    )
    .await
    {
        Either3::First(reason) => reason,
        _ => {
            if connection.is_connected() {
                connection.disconnect();
            }
            disconnected(connection).await
        }
    };
    device.disconnected();
    emit(device, handle, 0, Event::DISCONNECTED, reason);
    if let Some(command) = active.get() {
        let result = if command == DISCONNECT::IDENTIFIER.get_number() {
            Ok(())
        } else {
            Err(IoErrorKind::NotConnected)
        };
        device.complete(command, handle, result);
    }
}

async fn disconnected(connection: &Connection<'_, Pool>) -> u8 {
    loop {
        if let ConnectionEvent::Disconnected { reason } = connection.next().await {
            return reason.into_inner();
        }
    }
}

async fn finish_link(link: Option<Connection<'_, Pool>>) {
    if let Some(link) = link {
        if link.is_connected() {
            link.disconnect();
        }
        disconnected(&link).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_gatt<C: Controller>(
    device: &BluetoothDevice,
    connection: ConnectionHandle,
    client: &Client<'_, C>,
    services: &RefCell<Vec<ServiceHandle, DISCOVERY_LIMIT>>,
    subscriptions: &RefCell<Vec<u16, DISCOVERY_LIMIT>>,
    command: u8,
    request: Request,
    maximum_transmission_unit: u16,
) -> Result<()> {
    match request {
        Request::Services => {
            let found = client.services().await.map_err(map_error)?;
            for service in &found {
                let range = service.handle_range();
                let payload = Service {
                    first_handle: *range.start(),
                    last_handle: *range.end(),
                    uuid: uuid(service.uuid()),
                    reserved: 0,
                };
                emit(device, connection, command, Event::SERVICE, payload);
            }
            services.replace(found);
        }
        Request::Characteristics(handle) => {
            let service = services
                .borrow()
                .iter()
                .find(|s| *s.handle_range().start() == handle)
                .cloned()
                .ok_or(IoErrorKind::NotFound)?;
            for item in client
                .characteristics::<DISCOVERY_LIMIT>(&service)
                .await
                .map_err(map_error)?
            {
                let payload = Characteristic {
                    value_handle: item.handle,
                    last_handle: item.end_handle,
                    configuration_handle: item.cccd_handle.unwrap_or(0),
                    properties: item.props.as_gatt()[0],
                    uuid: uuid(item.uuid),
                };
                emit(device, connection, command, Event::CHARACTERISTIC, payload);
            }
        }
        Request::Descriptors(handle) => {
            let characteristic = find_characteristic(client, services, handle).await?;
            for descriptor in client
                .descriptors::<_, DISCOVERY_LIMIT>(&characteristic)
                .await
                .map_err(map_error)?
            {
                let payload = Descriptor {
                    handle: descriptor.handle(),
                    uuid: uuid(*descriptor.uuid()),
                    reserved: 0,
                };
                emit(device, connection, command, Event::DESCRIPTOR, payload);
            }
        }
        Request::Read(handle) => {
            let mut bytes = [0; VALUE_CAPACITY + 1];
            let length = client
                .read_handle(handle, &mut bytes)
                .await
                .map_err(map_error)?;
            if length > VALUE_CAPACITY {
                return Err(IoErrorKind::MessageTooLarge);
            }
            let mut value = AttributeValue {
                handle,
                length: length as u8,
                ..Default::default()
            };
            value.bytes[..length].copy_from_slice(&bytes[..length]);
            emit(device, connection, command, Event::VALUE, value);
        }
        Request::Write(value) => {
            let bytes = value.as_slice().ok_or(IoErrorKind::InvalidArgument)?;
            if bytes.len() + 3 > usize::from(maximum_transmission_unit) {
                return Err(IoErrorKind::MessageTooLarge);
            }
            client
                .write_handle(value.handle, bytes)
                .await
                .map_err(map_error)?;
        }
        Request::Notifications(request) => {
            let handle = request.characteristic.handle;
            let enable = request.enabled != 0;
            let index = subscriptions
                .borrow()
                .iter()
                .position(|entry| *entry == handle);
            if enable == index.is_some() {
                return Ok(());
            }
            if enable && subscriptions.borrow().is_full() {
                return Err(IoErrorKind::NoSpace);
            }
            let characteristic = find_characteristic(client, services, handle).await?;
            if !characteristic
                .props
                .any(&[trouble_host::prelude::CharacteristicProp::Notify])
            {
                return Err(IoErrorKind::Unsupported);
            }
            let configuration = characteristic.cccd_handle.ok_or(IoErrorKind::Unsupported)?;
            if let Some(index) = index {
                client
                    .write_handle(configuration, &[0, 0])
                    .await
                    .map_err(map_error)?;
                subscriptions.borrow_mut().swap_remove(index);
            } else {
                subscriptions
                    .borrow_mut()
                    .push(handle)
                    .map_err(|_| IoErrorKind::NoSpace)?;
                if let Err(error) = client.write_handle(configuration, &[1, 0]).await {
                    subscriptions.borrow_mut().pop();
                    return Err(map_error(error));
                }
            }
        }
        _ => return Err(IoErrorKind::ResourceBusy),
    }
    Ok(())
}

async fn find_characteristic<C: Controller>(
    client: &Client<'_, C>,
    services: &RefCell<Vec<ServiceHandle, DISCOVERY_LIMIT>>,
    handle: u16,
) -> Result<HostCharacteristic> {
    let service = services
        .borrow()
        .iter()
        .find(|s| s.handle_range().contains(&handle))
        .cloned()
        .ok_or(IoErrorKind::NotFound)?;
    client
        .characteristics::<DISCOVERY_LIMIT>(&service)
        .await
        .map_err(map_error)?
        .into_iter()
        .find(|item| item.handle == handle)
        .ok_or(IoErrorKind::NotFound)
}

fn emit(
    device: &BluetoothDevice,
    connection: ConnectionHandle,
    command: u8,
    kind: u8,
    value: impl file_system::PlainData,
) {
    device.emit(Event::new(kind, command, connection).with_payload(value));
}

fn uuid(source: trouble_host::prelude::Uuid) -> Uuid {
    use trouble_host::prelude::Uuid as HostUuid;
    let mut uuid = Uuid::default();
    let source: &[u8] = match &source {
        HostUuid::Uuid16(bytes) => bytes,
        HostUuid::Uuid32(bytes) => bytes,
        HostUuid::Uuid128(bytes) => bytes,
    };
    uuid.length = source.len() as u8;
    uuid.bytes[..source.len()].copy_from_slice(source);
    uuid
}

fn map_error<E: embedded_io::Error>(error: BleHostError<E>) -> IoErrorKind {
    use trouble_host::att::AttErrorCode;

    match error {
        BleHostError::Controller(error) => match error.kind() {
            embedded_io::ErrorKind::TimedOut => IoErrorKind::TimedOut,
            _ => IoErrorKind::InputOutput,
        },
        BleHostError::BleHost(error) => match error {
            Error::Timeout => IoErrorKind::TimedOut,
            Error::Disconnected => IoErrorKind::NotConnected,
            Error::NotFound
            | Error::Att(AttErrorCode::INVALID_HANDLE | AttErrorCode::ATTRIBUTE_NOT_FOUND) => {
                IoErrorKind::NotFound
            }
            Error::Att(
                AttErrorCode::READ_NOT_PERMITTED
                | AttErrorCode::WRITE_NOT_PERMITTED
                | AttErrorCode::INSUFFICIENT_AUTHENTICATION
                | AttErrorCode::INSUFFICIENT_ENCRYPTION,
            ) => IoErrorKind::PermissionDenied,
            Error::InvalidValue => IoErrorKind::InvalidArgument,
            Error::InsufficientSpace => IoErrorKind::NoSpace,
            Error::OutOfMemory => IoErrorKind::OutOfMemory,
            Error::Busy | Error::ConnectionLimitReached => IoErrorKind::ResourceBusy,
            Error::NotSupported => IoErrorKind::Unsupported,
            Error::Hci(bt_hci::param::Error::CONN_TIMEOUT) => IoErrorKind::TimedOut,
            _ => IoErrorKind::ProtocolError,
        },
    }
}
