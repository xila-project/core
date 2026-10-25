use core::time::Duration;
use drivers_native::wifi::*;
use file_system::{ControlCommand, DirectBaseOperations, Error};
use network::*;
use shared::AnyByLayout;

fn control<C: ControlCommand>(
    controller: &MockWifiController,
    _: C,
    input: &C::Input,
) -> file_system::Result<C::Output>
where
    C::Output: Default,
{
    let mut output = C::Output::default();
    controller.control_sync(
        C::IDENTIFIER,
        AnyByLayout::from(input),
        AnyByLayout::from_mutable(&mut output),
    )?;
    Ok(output)
}
fn mock() -> (MockWifiController, MockWifiRunner) {
    MockWifiController::new(MockWifiConfiguration {
        access_points: vec![MockWifiAccessPoint {
            access_point: WifiAccessPoint {
                ssid: WifiSsid::new(b"Xila demo").unwrap(),
                bssid: [2, 0, 0, 0, 0, 1],
                channel: 6,
                rssi: -42,
                authentication: WifiAuthentication::WPA2_PERSONAL,
            },
            credential: WifiPassword::new(b"xila-demo").unwrap(),
            outcome: WifiConnectionErrorKind::NONE,
        }],
        scan_latency: Duration::from_millis(10),
        connect_latency: Duration::from_millis(10),
    })
    .unwrap()
}

#[test]
fn controls_schedule_work_and_cancel_without_runner_threads() {
    let (controller, runner) = mock();
    control(
        &controller,
        START_WIFI_SCAN,
        &WifiScanConfiguration::default(),
    )
    .unwrap();
    assert_eq!(
        control(&controller, GET_WIFI_SCAN_STATE, &())
            .unwrap()
            .phase,
        WifiScanPhase::SCANNING
    );
    assert_eq!(
        control(&controller, GET_WIFI_SCAN_RESULT_COUNT, &())
            .unwrap()
            .count,
        0
    );
    let config = WifiClientConfiguration::new(
        b"Xila demo",
        WifiAuthentication::WPA2_PERSONAL,
        b"xila-demo",
    )
    .unwrap();
    assert_eq!(
        control(&controller, CONNECT_WIFI_CLIENT, &config),
        Err(Error::RessourceBusy)
    );
    control(&controller, SET_WIFI_RADIO_STATE, &WifiRadioState::OFF).unwrap();
    assert!(!control(&controller, IS_LINK_UP, &()).unwrap());
    assert_eq!(
        control(&controller, GET_WIFI_SCAN_STATE, &())
            .unwrap()
            .phase,
        WifiScanPhase::IDLE
    );
    control(&controller, SET_WIFI_RADIO_STATE, &WifiRadioState::ON).unwrap();
    control(&controller, CONNECT_WIFI_CLIENT, &config).unwrap();
    drop(runner);
    assert_eq!(
        control(&controller, GET_WIFI_CONNECTION_STATE, &()).unwrap(),
        WifiConnectionState::DISCONNECTED
    );
    assert_eq!(
        control(&controller, GET_WIFI_LAST_CONNECTION_ERROR, &()).unwrap(),
        WifiConnectionErrorKind::DRIVER
    );
    assert_eq!(
        control(
            &controller,
            START_WIFI_SCAN,
            &WifiScanConfiguration::default()
        ),
        Err(Error::InputOutput)
    );
}

extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();

#[task::test]
async fn vfs_scan_associate_fail_disconnect_and_cancel() {
    use virtual_file_system::File;
    let standard = testing::initialize(false, false).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;
    let manager = Box::leak(Box::new(network::initialize(
        context.task_manager,
        context.virtual_file_system,
        context.time_manager,
        &drivers_shared::devices::RandomDevice,
    )));
    let (controller, runner) = mock();
    let (packets, _) = network::create_loopback_device();
    manager
        .mount_interface(task, "wlan0", packets, controller.clone(), None)
        .await
        .unwrap();
    context
        .task_manager
        .spawn(task, "Mock Wi-Fi", None, move |_| async move {
            runner.run().await
        })
        .await
        .unwrap();
    let mut directory =
        virtual_file_system::Directory::open(context.virtual_file_system, task, "/devices/network")
            .await
            .unwrap();
    let mut found = false;
    while let Some(entry) = directory.read().await.unwrap() {
        found |= entry.name == "wlan0";
    }
    assert!(found);
    directory.close(context.virtual_file_system).await.unwrap();
    let mut file = File::open(
        context.virtual_file_system,
        task,
        "/devices/network/wlan0",
        file_system::AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();
    assert_eq!(
        file.control(GET_KIND, &()).await.unwrap(),
        InterfaceKind::WiFi
    );
    assert!(!file.control(IS_LINK_UP, &()).await.unwrap());
    file.control(START_WIFI_SCAN, &WifiScanConfiguration::default())
        .await
        .unwrap();
    assert_eq!(
        file.control(GET_WIFI_SCAN_STATE, &()).await.unwrap().phase,
        WifiScanPhase::SCANNING
    );
    task::sleep(Duration::from_millis(60)).await;
    let count = file.control(GET_WIFI_SCAN_RESULT_COUNT, &()).await.unwrap();
    assert_eq!(count.count, 1);
    let ap = file
        .control(
            GET_WIFI_SCAN_RESULT,
            &WifiScanResultRequest {
                generation: count.generation,
                index: 0,
                reserved: [0; 2],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (ap.channel, ap.rssi, ap.bssid),
        (6, -42, [2, 0, 0, 0, 0, 1])
    );
    assert!(
        file.control(
            GET_WIFI_SCAN_RESULT,
            &WifiScanResultRequest {
                generation: count.generation.wrapping_sub(1),
                index: 0,
                reserved: [0; 2]
            }
        )
        .await
        .is_err()
    );
    let wrong = WifiClientConfiguration::new(
        b"Xila demo",
        WifiAuthentication::WPA2_PERSONAL,
        b"wrong-key",
    )
    .unwrap();
    file.control(CONNECT_WIFI_CLIENT, &wrong).await.unwrap();
    task::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        file.control(GET_WIFI_LAST_CONNECTION_ERROR, &())
            .await
            .unwrap(),
        WifiConnectionErrorKind::AUTHENTICATION
    );
    assert!(!file.control(IS_LINK_UP, &()).await.unwrap());
    assert!(file.control(GET_WIFI_CONNECTION_INFO, &()).await.is_err());
    let good = WifiClientConfiguration::new(
        b"Xila demo",
        WifiAuthentication::WPA2_PERSONAL,
        b"xila-demo",
    )
    .unwrap();
    file.control(CONNECT_WIFI_CLIENT, &good).await.unwrap();
    task::sleep(Duration::from_millis(60)).await;
    assert!(file.control(IS_LINK_UP, &()).await.unwrap());
    assert_eq!(
        file.control(GET_WIFI_CONNECTION_INFO, &()).await.unwrap(),
        ap
    );
    // Latest completed results stay readable while a filtered scan runs.
    file.control(
        START_WIFI_SCAN,
        &WifiScanConfiguration {
            channel: 11,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        file.control(GET_WIFI_SCAN_RESULT_COUNT, &()).await.unwrap(),
        count
    );
    task::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        file.control(GET_WIFI_SCAN_RESULT_COUNT, &())
            .await
            .unwrap()
            .count,
        0
    );
    file.control(DISCONNECT_WIFI_CLIENT, &()).await.unwrap();
    task::sleep(Duration::from_millis(60)).await;
    assert!(!file.control(IS_LINK_UP, &()).await.unwrap());
    // Turning radio off while association waits cannot produce a stale link-up.
    file.control(CONNECT_WIFI_CLIENT, &good).await.unwrap();
    file.control(SET_WIFI_RADIO_STATE, &WifiRadioState::OFF)
        .await
        .unwrap();
    task::sleep(Duration::from_millis(60)).await;
    assert!(!file.control(IS_LINK_UP, &()).await.unwrap());
    assert_eq!(
        file.control(GET_WIFI_CONNECTION_STATE, &()).await.unwrap(),
        WifiConnectionState::DISCONNECTED
    );
    file.control(SET_WIFI_RADIO_STATE, &WifiRadioState::ON)
        .await
        .unwrap();
    file.control(CONNECT_WIFI_CLIENT, &good).await.unwrap();
    task::sleep(Duration::from_millis(60)).await;
    controller.simulate_disconnect(WifiConnectionErrorKind::ASSOCIATION);
    assert!(!file.control(IS_LINK_UP, &()).await.unwrap());
    file.close(context.virtual_file_system).await.unwrap();
}

#[test]
fn malformed_payloads_do_not_start_operations() {
    let (controller, _runner) = mock();
    assert_eq!(
        control(&controller, SET_WIFI_RADIO_STATE, &WifiRadioState(255)),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        control(
            &controller,
            START_WIFI_SCAN,
            &WifiScanConfiguration {
                passive: 2,
                ..Default::default()
            }
        ),
        Err(Error::InvalidParameter)
    );
    let mut configuration = WifiClientConfiguration::new(
        b"Xila demo",
        WifiAuthentication::WPA2_PERSONAL,
        b"xila-demo",
    )
    .unwrap();
    configuration.credential.length = 255;
    assert_eq!(
        control(&controller, CONNECT_WIFI_CLIENT, &configuration),
        Err(Error::InvalidParameter)
    );
    configuration.credential.length = 9;
    configuration.authentication = WifiAuthentication(127);
    assert_eq!(
        control(&controller, CONNECT_WIFI_CLIENT, &configuration),
        Err(Error::UnsupportedOperation)
    );
    assert_eq!(
        control(&controller, GET_WIFI_CONNECTION_STATE, &()).unwrap(),
        WifiConnectionState::DISCONNECTED
    );
    assert_eq!(
        control(&controller, GET_WIFI_SCAN_STATE, &())
            .unwrap()
            .phase,
        WifiScanPhase::IDLE
    );
    assert!(
        control(
            &controller,
            GET_WIFI_SCAN_RESULT,
            &WifiScanResultRequest::default()
        )
        .is_err()
    );
    assert!(control(&controller, GET_WIFI_CONNECTION_INFO, &()).is_err());
}
