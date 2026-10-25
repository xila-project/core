use super::*;
use file_system::{ControlCommand, define_command};

define_command!(GET_WIFI_CAPABILITIES, Read, b'w', 0, (), WifiCapabilities);
define_command!(GET_WIFI_RADIO_STATE, Read, b'w', 1, (), WifiRadioState);
define_command!(SET_WIFI_RADIO_STATE, Write, b'w', 2, WifiRadioState, ());
define_command!(START_WIFI_SCAN, Write, b'w', 3, WifiScanConfiguration, ());
define_command!(GET_WIFI_SCAN_STATE, Read, b'w', 4, (), WifiScanState);
define_command!(
    GET_WIFI_SCAN_RESULT_COUNT,
    Read,
    b'w',
    5,
    (),
    WifiScanResultCount
);
// A generation mismatch returns InvalidParameter; fetch the count and restart enumeration.
define_command!(
    GET_WIFI_SCAN_RESULT,
    Read,
    b'w',
    6,
    WifiScanResultRequest,
    WifiAccessPoint
);
define_command!(
    CONNECT_WIFI_CLIENT,
    Write,
    b'w',
    7,
    WifiClientConfiguration,
    ()
);
define_command!(DISCONNECT_WIFI_CLIENT, Write, b'w', 8, (), ());
define_command!(
    GET_WIFI_CONNECTION_STATE,
    Read,
    b'w',
    9,
    (),
    WifiConnectionState
);
// Returns NotFound unless associated.
define_command!(
    GET_WIFI_CONNECTION_INFO,
    Read,
    b'w',
    10,
    (),
    WifiConnectionInfo
);
define_command!(
    GET_WIFI_LAST_CONNECTION_ERROR,
    Read,
    b'w',
    11,
    (),
    WifiConnectionErrorKind
);
