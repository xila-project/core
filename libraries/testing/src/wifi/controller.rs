use super::MockWifiRunner;
use core::{
    cell::RefCell,
    task::{Context, Poll},
    time::Duration,
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice, Error,
    MountOperations, Result,
};
use network::*;
use shared::AnyByLayout;
use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    signal::Signal,
};

pub const MAXIMUM_MOCK_ACCESS_POINTS: usize = 16;

/// Fixture credentials belong to the test harness, never to a saved-profile store.
#[derive(Debug)]
pub struct MockWifiAccessPoint {
    pub access_point: WifiAccessPoint,
    pub credential: WifiPassword,
    pub outcome: WifiConnectionErrorKind,
}
#[derive(Debug)]
pub struct MockWifiConfiguration {
    pub access_points: Vec<MockWifiAccessPoint>,
    pub scan_latency: Duration,
    pub connect_latency: Duration,
}
impl MockWifiConfiguration {
    /// Public, synthetic fixtures for native UI testing only.
    pub fn demo() -> Self {
        Self {
            access_points: vec![
                MockWifiAccessPoint {
                    access_point: WifiAccessPoint {
                        ssid: WifiSsid::new(b"Xila demo").unwrap(),
                        bssid: [2, 0, 0, 0, 0, 1],
                        channel: 6,
                        rssi: -42,
                        authentication: WifiAuthentication::WPA2_PERSONAL,
                    },
                    credential: WifiPassword::new(b"xila-demo").unwrap(),
                    outcome: WifiConnectionErrorKind::NONE,
                },
                MockWifiAccessPoint {
                    access_point: WifiAccessPoint {
                        ssid: WifiSsid::new(b"Xila open").unwrap(),
                        bssid: [2, 0, 0, 0, 0, 2],
                        channel: 11,
                        rssi: -65,
                        authentication: WifiAuthentication::OPEN,
                    },
                    credential: WifiPassword::default(),
                    outcome: WifiConnectionErrorKind::NONE,
                },
            ],
            scan_latency: Duration::from_millis(250),
            connect_latency: Duration::from_millis(500),
        }
    }
}

pub(super) enum Pending {
    Scan(WifiScanConfiguration),
    Connect(WifiClientConfiguration),
    Disconnect,
}
pub(super) struct State {
    pub configuration: MockWifiConfiguration,
    pub radio: WifiRadioState,
    pub scan: WifiScanState,
    pub results: [WifiAccessPoint; MAXIMUM_MOCK_ACCESS_POINTS],
    pub result_count: u16,
    pub connection: WifiConnectionState,
    pub info: WifiConnectionInfo,
    pub error: WifiConnectionErrorKind,
    pub pending: Option<Pending>,
    pub revision: u64,
    pub alive: bool,
}
impl State {
    pub fn cancel(&mut self) {
        self.pending = None; // drops and clears an ephemeral connection credential
        self.revision = self.revision.wrapping_add(1);
        if self.scan.phase == WifiScanPhase::SCANNING {
            self.scan.phase = WifiScanPhase::IDLE;
        }
        self.connection = WifiConnectionState::DISCONNECTED;
        self.info = WifiConnectionInfo::default();
    }
    pub fn complete(&mut self) {
        match self.pending.take() {
            Some(Pending::Scan(configuration)) => {
                let mut count = 0;
                for fixture in &self.configuration.access_points {
                    if count
                        >= usize::from(configuration.result_limit).min(MAXIMUM_MOCK_ACCESS_POINTS)
                    {
                        break;
                    }
                    if configuration.channel == 0
                        || configuration.channel == fixture.access_point.channel
                    {
                        self.results[count] = fixture.access_point;
                        count += 1;
                    }
                }
                self.result_count = count as u16;
                self.scan.generation = self.scan.generation.wrapping_add(1);
                self.scan.phase = WifiScanPhase::COMPLETED;
            }
            Some(Pending::Connect(configuration)) => {
                let fixture = self.configuration.access_points.iter().find(|fixture| {
                    fixture.access_point.ssid.as_bytes().ok() == configuration.ssid.as_bytes().ok()
                        && (configuration.use_bssid == 0
                            || fixture.access_point.bssid == configuration.bssid)
                        && (configuration.channel == 0
                            || fixture.access_point.channel == configuration.channel)
                });
                self.error = match fixture {
                    None => WifiConnectionErrorKind::ACCESS_POINT_NOT_FOUND,
                    Some(fixture)
                        if fixture.access_point.authentication != configuration.authentication =>
                    {
                        WifiConnectionErrorKind::UNSUPPORTED_CONFIGURATION
                    }
                    Some(fixture)
                        if fixture.credential.as_bytes().ok()
                            != configuration.credential.as_bytes().ok() =>
                    {
                        WifiConnectionErrorKind::AUTHENTICATION
                    }
                    Some(fixture) => fixture.outcome,
                };
                if self.error == WifiConnectionErrorKind::NONE {
                    self.info = fixture.unwrap().access_point;
                    self.connection = WifiConnectionState::CONNECTED;
                } else {
                    self.info = WifiConnectionInfo::default();
                    self.connection = WifiConnectionState::DISCONNECTED;
                }
            }
            Some(Pending::Disconnect) => self.cancel(),
            None => (),
        }
    }
}
pub(super) struct Shared {
    pub state: Mutex<CriticalSectionRawMutex, RefCell<State>>,
    pub wake: Signal<CriticalSectionRawMutex, ()>,
}
#[derive(Clone)]
pub struct MockWifiController {
    pub(super) shared: Arc<Shared>,
}
impl MockWifiController {
    pub fn new(mut configuration: MockWifiConfiguration) -> Result<(Self, MockWifiRunner)> {
        if configuration.access_points.len() > MAXIMUM_MOCK_ACCESS_POINTS {
            return Err(Error::InvalidParameter);
        }
        for fixture in &configuration.access_points {
            fixture.access_point.ssid.as_bytes()?;
            if fixture.access_point.channel == 0 || fixture.access_point.channel > 196 {
                return Err(Error::InvalidParameter);
            }
            // Extended/unsupported APs can be scanned but not connected.
            if fixture.access_point.authentication != WifiAuthentication::UNSUPPORTED {
                WifiClientConfiguration::new(
                    fixture.access_point.ssid.as_bytes()?,
                    fixture.access_point.authentication,
                    fixture.credential.as_bytes()?,
                )?;
            }
        }
        // Do not retain an arbitrarily oversized caller Vec capacity.
        configuration.access_points = configuration.access_points.into_boxed_slice().into_vec();
        let shared = Arc::new(Shared {
            state: Mutex::new(RefCell::new(State {
                configuration,
                radio: WifiRadioState::ON,
                scan: WifiScanState::default(),
                results: [WifiAccessPoint::default(); MAXIMUM_MOCK_ACCESS_POINTS],
                result_count: 0,
                connection: WifiConnectionState::DISCONNECTED,
                info: WifiConnectionInfo::default(),
                error: WifiConnectionErrorKind::NONE,
                pending: None,
                revision: 0,
                alive: true,
            })),
            wake: Signal::new(),
        });
        Ok((
            Self {
                shared: shared.clone(),
            },
            MockWifiRunner { shared },
        ))
    }
    /// Inject a link-loss event; cancels pending work as a radio event would.
    pub fn simulate_disconnect(&self, error: WifiConnectionErrorKind) {
        self.shared.state.lock(|state| {
            let mut state = state.borrow_mut();
            state.cancel();
            state.error = error;
        });
        self.shared.wake.signal(());
    }
    fn execute(
        &self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        let result = self.shared.state.lock(|state| {
            let mut state = state.borrow_mut();
            match command {
                GET_KIND::IDENTIFIER => *GET_KIND::cast_output(output)? = InterfaceKind::WiFi,
                IS_LINK_UP::IDENTIFIER => {
                    *IS_LINK_UP::cast_output(output)? = state.alive
                        && state.radio == WifiRadioState::ON
                        && state.connection == WifiConnectionState::CONNECTED
                }
                GET_WIFI_CAPABILITIES::IDENTIFIER => {
                    *GET_WIFI_CAPABILITIES::cast_output(output)? = WifiCapabilities {
                        station: 1,
                        access_point: 0,
                        authentication_mask: 0b1111,
                        maximum_scan_results: MAXIMUM_MOCK_ACCESS_POINTS as u8,
                    }
                }
                GET_WIFI_RADIO_STATE::IDENTIFIER => {
                    *GET_WIFI_RADIO_STATE::cast_output(output)? = state.radio
                }
                SET_WIFI_RADIO_STATE::IDENTIFIER => {
                    let radio = *SET_WIFI_RADIO_STATE::cast_input(input)?;
                    if radio != WifiRadioState::ON && radio != WifiRadioState::OFF {
                        return Err(Error::InvalidParameter);
                    }
                    if !state.alive {
                        return Err(Error::InputOutput);
                    }
                    state.radio = radio;
                    if radio == WifiRadioState::OFF {
                        state.cancel();
                        state.error = WifiConnectionErrorKind::NONE;
                    }
                }
                GET_WIFI_SCAN_STATE::IDENTIFIER => {
                    *GET_WIFI_SCAN_STATE::cast_output(output)? = state.scan
                }
                GET_WIFI_SCAN_RESULT_COUNT::IDENTIFIER => {
                    *GET_WIFI_SCAN_RESULT_COUNT::cast_output(output)? = WifiScanResultCount {
                        generation: state.scan.generation,
                        count: state.result_count,
                        reserved: [0; 2],
                    }
                }
                GET_WIFI_SCAN_RESULT::IDENTIFIER => {
                    let request = GET_WIFI_SCAN_RESULT::cast_input(input)?;
                    if request.generation != state.scan.generation
                        || request.index >= state.result_count
                    {
                        return Err(Error::InvalidParameter);
                    }
                    *GET_WIFI_SCAN_RESULT::cast_output(output)? =
                        state.results[request.index as usize];
                }
                GET_WIFI_CONNECTION_STATE::IDENTIFIER => {
                    *GET_WIFI_CONNECTION_STATE::cast_output(output)? = state.connection
                }
                GET_WIFI_CONNECTION_INFO::IDENTIFIER => {
                    if state.connection != WifiConnectionState::CONNECTED {
                        return Err(Error::NotFound);
                    }
                    *GET_WIFI_CONNECTION_INFO::cast_output(output)? = state.info;
                }
                GET_WIFI_LAST_CONNECTION_ERROR::IDENTIFIER => {
                    *GET_WIFI_LAST_CONNECTION_ERROR::cast_output(output)? = state.error
                }
                START_WIFI_SCAN::IDENTIFIER
                | CONNECT_WIFI_CLIENT::IDENTIFIER
                | DISCONNECT_WIFI_CLIENT::IDENTIFIER => {
                    if !state.alive {
                        return Err(Error::InputOutput);
                    }
                    if state.radio != WifiRadioState::ON {
                        return Err(Error::InvalidParameter);
                    }
                    if state.pending.is_some() {
                        return Err(Error::RessourceBusy);
                    }
                    state.pending = Some(match command {
                        START_WIFI_SCAN::IDENTIFIER => {
                            let configuration = *START_WIFI_SCAN::cast_input(input)?;
                            configuration.validate()?;
                            state.scan.phase = WifiScanPhase::SCANNING;
                            Pending::Scan(configuration)
                        }
                        CONNECT_WIFI_CLIENT::IDENTIFIER => {
                            let configuration = CONNECT_WIFI_CLIENT::cast_input(input)?;
                            configuration.validate()?;
                            state.connection = WifiConnectionState::CONNECTING;
                            state.info = WifiConnectionInfo::default();
                            state.error = WifiConnectionErrorKind::NONE;
                            Pending::Connect(configuration.clone())
                        }
                        _ => {
                            state.connection = WifiConnectionState::DISCONNECTING;
                            Pending::Disconnect
                        }
                    });
                    state.revision = state.revision.wrapping_add(1);
                }
                _ => return Err(Error::UnsupportedOperation),
            }
            Ok(())
        });
        // Signal outside the critical section. Read commands never restart a delay.
        if result.is_ok()
            && [
                START_WIFI_SCAN::IDENTIFIER,
                CONNECT_WIFI_CLIENT::IDENTIFIER,
                DISCONNECT_WIFI_CLIENT::IDENTIFIER,
                SET_WIFI_RADIO_STATE::IDENTIFIER,
            ]
            .contains(&command)
        {
            self.shared.wake.signal(());
        }
        result
    }
}
impl DirectBaseOperations for MockWifiController {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: file_system::Size) -> Poll<Result<usize>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }
    fn write(&self, _: &mut Context<'_>, _: &[u8], _: file_system::Size) -> Poll<Result<usize>> {
        Poll::Ready(Err(Error::UnsupportedOperation))
    }
    fn control(
        &self,
        _: &mut Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        Poll::Ready(self.execute(command, input, output))
    }
}
impl MountOperations for MockWifiController {}
impl DirectCharacterDevice for MockWifiController {}
