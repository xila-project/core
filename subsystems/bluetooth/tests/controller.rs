#![cfg(target_os = "linux")]

use bluetooth::*;
use drivers_core::bluetooth::{Attribute, AttributeData, Peripheral, Simulation};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, with_timeout};
use file_system::{
    AccessFlags, ControlCommand, Flags, IoErrorKind, Path, ReadinessWaiter, ReadyEvents,
};
use virtual_file_system::File;

const PEER: Address = Address {
    bytes: [1, 2, 3, 4, 5, 6],
    kind: Address::PUBLIC,
};
const SERVICE_UUID: Uuid = Uuid::short(0x180f);
const CHARACTERISTIC_UUID: Uuid = Uuid::short(0x2a19);
const PROPERTIES: u8 = 0x02 | 0x08 | 0x10;

#[task::test]
async fn simulated_controller_through_the_virtual_file_system() {
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    log::initialize(&testing_support::Logger).unwrap();
    let task = task_manager.get_current_task_identifier().await;
    let file_system = virtual_file_system::initialize(
        task_manager,
        users_manager,
        time_manager,
        memory_file_system::FileSystem::new(),
    )
    .unwrap();
    file_system
        .create_directory(task, &Path::DEVICES)
        .await
        .unwrap();

    let device = Box::leak(Box::new(BluetoothDevice::new()));
    mount(&file_system, task, device).await.unwrap();

    let mut attributes = [
        Attribute {
            handle: 1,
            data: AttributeData::Service {
                uuid: SERVICE_UUID,
                last_handle: 4,
            },
        },
        Attribute {
            handle: 2,
            data: AttributeData::Characteristic {
                uuid: CHARACTERISTIC_UUID,
                properties: PROPERTIES,
                value_handle: 3,
            },
        },
        Attribute {
            handle: 3,
            data: AttributeData::Value {
                uuid: CHARACTERISTIC_UUID,
                properties: PROPERTIES,
                value: value(25),
            },
        },
        Attribute {
            handle: 4,
            data: AttributeData::ClientConfiguration { value_handle: 3 },
        },
    ];
    let mut advertising = AdvertisingData {
        length: 3,
        ..Default::default()
    };
    advertising.bytes[..3].copy_from_slice(&[2, 1, 6]);
    let mut peripherals = [Peripheral {
        address: PEER,
        advertising,
        rssi: -38,
        advertisement_delay: Some(3),
        connection_delay: Some(2),
        attribute_delay: 1,
        attributes: &mut attributes,
    }];
    let controller = Simulation::new([6, 5, 4, 3, 2, 1], &mut peripherals).unwrap();

    let flags = Flags::new(AccessFlags::READ_WRITE, None, None);
    let exercise = async {
        let mut file = File::open(&file_system, task, DEVICE_PATH, flags)
            .await
            .unwrap();
        assert_eq!(
            File::open(&file_system, task, DEVICE_PATH, flags)
                .await
                .err(),
            Some(IoErrorKind::ResourceBusy)
        );
        assert_eq!(
            file.control(GET_CAPABILITIES, &())
                .await
                .unwrap()
                .connection_limit,
            1
        );

        file.control(START_SCAN, &ScanConfiguration::default())
            .await
            .unwrap();
        assert_eq!(
            file.control(STOP_SCAN, &()).await,
            Err(IoErrorKind::ResourceBusy)
        );
        let event = next_event(&mut file).await;
        completion(event, START_SCAN).unwrap();
        assert_eq!(file.control(GET_STATE, &()).await.unwrap().scanning, 1);
        assert!(matches!(
            select(next_event(&mut file), core::future::ready(())).await,
            Either::Second(())
        ));

        let readiness = file.as_synchronous_file_mut().readiness();
        let waiter = core::pin::pin!(ReadinessWaiter::default());
        let ready = core::future::poll_fn(|context| {
            waiter.as_ref().register(context.waker()).unwrap();
            if readiness.events(ReadyEvents::READ).unwrap().0.is_empty() {
                core::task::Poll::Pending
            } else {
                core::task::Poll::Ready(())
            }
        });
        let report = async {
            task::yield_now().await;
            controller.advance(3);
            core::future::pending::<()>().await;
        };
        assert!(matches!(select(ready, report).await, Either::First(())));
        let result: ScanResult = next_event(&mut file)
            .await
            .payload(Event::SCAN_RESULT)
            .unwrap();
        assert_eq!(result.peer, PEER);
        assert_eq!(result.rssi, -38);
        assert_eq!(result.data, advertising);

        for _ in 0..20 {
            controller.advertise_after(PEER, 1).unwrap();
        }
        controller.advance(1);
        task::yield_now().await;
        file.control(STOP_SCAN, &()).await.unwrap();
        for _ in 0..15 {
            assert_eq!(next_event(&mut file).await.kind, Event::SCAN_RESULT);
        }
        let first = next_event(&mut file).await;
        let second = next_event(&mut file).await;
        let (lost, completed) = if first.kind == Event::OVERFLOW {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(lost.payload(Event::OVERFLOW), Some(5u32));
        completion(completed, STOP_SCAN).unwrap();

        file.control(START_ADVERTISING, &advertising).await.unwrap();
        completion(next_event(&mut file).await, START_ADVERTISING).unwrap();
        assert!(controller.is_advertising());
        file.control(STOP_ADVERTISING, &()).await.unwrap();
        completion(next_event(&mut file).await, STOP_ADVERTISING).unwrap();
        assert!(!controller.is_advertising());

        let missing = ConnectionRequest {
            timeout_milliseconds: 20,
            peer: Address {
                bytes: [1; 6],
                ..PEER
            },
            reserved: 0,
        };
        file.control(CONNECT, &missing).await.unwrap();
        assert_eq!(
            completion(next_event(&mut file).await, CONNECT),
            Err(IoErrorKind::TimedOut)
        );

        let request = ConnectionRequest {
            timeout_milliseconds: 1000,
            peer: PEER,
            reserved: 0,
        };
        file.control(CONNECT, &request).await.unwrap();
        let event = stepped_event(&mut file, &controller).await;
        completion(event, CONNECT).unwrap();
        let connection = event.connection;
        assert_ne!(connection, ConnectionHandle::NONE);
        assert_eq!(
            file.control(DISCONNECT, &ConnectionHandle(connection.0 + 1))
                .await,
            Err(IoErrorKind::NotConnected)
        );

        file.control(DISCOVER_SERVICES, &connection).await.unwrap();
        let service: Service = stepped_event(&mut file, &controller)
            .await
            .payload(Event::SERVICE)
            .unwrap();
        assert_eq!(
            (service.first_handle, service.last_handle, service.uuid),
            (1, 4, SERVICE_UUID)
        );
        completion(next_event(&mut file).await, DISCOVER_SERVICES).unwrap();

        let service_request = AttributeRequest {
            connection,
            handle: service.first_handle,
            reserved: 0,
        };
        file.control(DISCOVER_CHARACTERISTICS, &service_request)
            .await
            .unwrap();
        let characteristic: Characteristic = stepped_event(&mut file, &controller)
            .await
            .payload(Event::CHARACTERISTIC)
            .unwrap();
        assert_eq!(characteristic.value_handle, 3);
        assert_eq!(characteristic.configuration_handle, 4);
        assert_eq!(characteristic.uuid, CHARACTERISTIC_UUID);
        completion(next_event(&mut file).await, DISCOVER_CHARACTERISTICS).unwrap();

        let attribute = AttributeRequest {
            handle: characteristic.value_handle,
            ..service_request
        };
        file.control(DISCOVER_DESCRIPTORS, &attribute)
            .await
            .unwrap();
        let descriptor: Descriptor = stepped_event(&mut file, &controller)
            .await
            .payload(Event::DESCRIPTOR)
            .unwrap();
        assert_eq!(
            (descriptor.handle, descriptor.uuid),
            (4, Uuid::short(0x2902))
        );
        completion(next_event(&mut file).await, DISCOVER_DESCRIPTORS).unwrap();

        let unknown = AttributeRequest {
            handle: 100,
            ..attribute
        };
        file.control(READ_ATTRIBUTE, &unknown).await.unwrap();
        assert_eq!(
            completion(stepped_event(&mut file, &controller).await, READ_ATTRIBUTE),
            Err(IoErrorKind::NotFound)
        );
        assert_eq!(
            read_value(&mut file, &controller, &attribute).await,
            value(25)
        );

        let subscription = NotificationRequest {
            characteristic: attribute,
            enabled: 1,
            reserved: [0; 3],
        };
        file.control(SET_NOTIFICATIONS, &subscription)
            .await
            .unwrap();
        completion(
            stepped_event(&mut file, &controller).await,
            SET_NOTIFICATIONS,
        )
        .unwrap();
        let update = WriteRequest {
            connection,
            value: value(71),
        };
        file.control(WRITE_ATTRIBUTE, &update).await.unwrap();
        let first = stepped_event(&mut file, &controller).await;
        let second = next_event(&mut file).await;
        let (notification, completed) = if first.kind == Event::NOTIFICATION {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(notification.payload(Event::NOTIFICATION), Some(value(71)));
        completion(completed, WRITE_ATTRIBUTE).unwrap();
        assert_eq!(
            read_value(&mut file, &controller, &attribute).await,
            value(71)
        );

        file.control(DISCONNECT, &connection).await.unwrap();
        let disconnected = next_event(&mut file).await;
        assert_eq!(disconnected.kind, Event::DISCONNECTED);
        assert_eq!(disconnected.connection, connection);
        completion(next_event(&mut file).await, DISCONNECT).unwrap();
        assert_eq!(
            file.control(GET_STATE, &()).await.unwrap().connection,
            ConnectionHandle::NONE
        );
        assert_eq!(
            file.control(READ_ATTRIBUTE, &attribute).await,
            Err(IoErrorKind::NotConnected)
        );

        // Closing during a delayed connection cancels the pending request.
        file.control(CONNECT, &request).await.unwrap();
        task::yield_now().await;
        file.close(&file_system).await.unwrap();
        let mut file = loop {
            match File::open(&file_system, task, DEVICE_PATH, flags).await {
                Ok(reopened) => break reopened,
                Err(IoErrorKind::ResourceBusy) => task::yield_now().await,
                Err(error) => panic!("reopening Bluetooth device: {error:?}"),
            }
        };
        assert_eq!(
            file.control(GET_STATE, &()).await.unwrap(),
            State::default()
        );
        file.control(CONNECT, &request).await.unwrap();
        let event = stepped_event(&mut file, &controller).await;
        completion(event, CONNECT).unwrap();
        assert_ne!(event.connection, connection);
        assert_eq!(
            file.control(DISCONNECT, &connection).await,
            Err(IoErrorKind::NotConnected)
        );

        controller.disconnect_after(1, 0x08).unwrap();
        let disconnected = stepped_event(&mut file, &controller).await;
        assert_eq!(disconnected.connection, event.connection);
        assert_eq!(disconnected.payload(Event::DISCONNECTED), Some(0x08u8));
        file.close(&file_system).await.unwrap();
    };
    match with_timeout(
        Duration::from_secs(3),
        select(device.run(controller.controller()), exercise),
    )
    .await
    .expect("Bluetooth scenario timed out")
    {
        Either::First(result) => panic!("Bluetooth runner exited: {result:?}"),
        Either::Second(()) => {}
    }
}

fn completion<C: ControlCommand>(event: Event, _: C) -> Result<(), IoErrorKind> {
    assert_eq!(event.kind, Event::COMPLETION);
    assert_eq!(event.command, C::IDENTIFIER.get_number());
    event.completion()
}

async fn next_event(file: &mut File<'_>) -> Event {
    let mut event = Event::default();
    assert_eq!(
        file.read(event.as_mutable_bytes()).await.unwrap(),
        size_of::<Event>()
    );
    event
}

async fn stepped_event(file: &mut File<'_>, controller: &Simulation<'_>) -> Event {
    let tick = async {
        loop {
            task::yield_now().await;
            controller.advance(1);
        }
    };
    match select(next_event(file), tick).await {
        Either::First(event) => event,
        Either::Second(()) => unreachable!(),
    }
}

async fn read_value(
    file: &mut File<'_>,
    controller: &Simulation<'_>,
    attribute: &AttributeRequest,
) -> AttributeValue {
    file.control(READ_ATTRIBUTE, attribute).await.unwrap();
    let result = stepped_event(file, controller)
        .await
        .payload(Event::VALUE)
        .unwrap();
    completion(next_event(file).await, READ_ATTRIBUTE).unwrap();
    result
}

fn value(byte: u8) -> AttributeValue {
    let mut value = AttributeValue {
        handle: 3,
        length: 1,
        ..Default::default()
    };
    value.bytes[0] = byte;
    value
}
