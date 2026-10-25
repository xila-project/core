use file_system::ControlCommand;
use network::*;

#[test]
fn bounded_names_and_credentials_are_validated() {
    assert!(WifiSsid::new(&[]).is_err());
    assert!(WifiSsid::new(&[b'a'; 33]).is_err());
    assert_eq!(
        WifiSsid::new(&[0xff; 32]).unwrap().as_bytes().unwrap(),
        &[0xff; 32]
    );
    let mut config =
        WifiClientConfiguration::new(b"xila", WifiAuthentication::WPA2_PERSONAL, b"password")
            .unwrap();
    assert!(config.validate().is_ok());
    config.credential.length = 65;
    assert!(config.validate().is_err());
    assert!(WifiClientConfiguration::new(b"xila", WifiAuthentication::OPEN, b"secret").is_err());
    assert!(
        WifiClientConfiguration::new(b"xila", WifiAuthentication::WPA2_PERSONAL, b"short").is_err()
    );
    assert!(
        WifiClientConfiguration::new(b"xila", WifiAuthentication::WPA2_PERSONAL, &[b'f'; 64])
            .is_ok()
    );
    assert!(
        WifiClientConfiguration::new(b"xila", WifiAuthentication::WPA2_PERSONAL, &[b'z'; 64])
            .is_err()
    );
    assert!(WifiClientConfiguration::new(b"xila", WifiAuthentication::WPA3_PERSONAL, b"a").is_ok());
    assert!(!format!("{:?}", config).contains("password"));
}

#[test]
fn control_payloads_have_fixed_layouts() {
    assert_eq!(size_of::<WifiSsid>(), 33);
    assert_eq!(size_of::<WifiPassword>(), 65);
    assert_eq!(size_of::<WifiAccessPoint>(), 42);
    assert_eq!(size_of::<WifiClientConfiguration>(), 107);
    assert_eq!(size_of::<WifiScanState>(), 8);
    assert_eq!(size_of::<WifiScanResultRequest>(), 8);
}

#[test]
fn all_network_command_identifiers_are_unique() {
    let identifiers = [
        GET_KIND::IDENTIFIER,
        SET_STATE::IDENTIFIER,
        GET_STATE::IDENTIFIER,
        GET_HARDWARE_ADDRESS::IDENTIFIER,
        SET_HARDWARE_ADDRESS::IDENTIFIER,
        GET_MAXIMUM_TRANSMISSION_UNIT::IDENTIFIER,
        GET_MAXIMUM_BURST_SIZE::IDENTIFIER,
        IS_LINK_UP::IDENTIFIER,
        GET_ROUTE_COUNT::IDENTIFIER,
        GET_ROUTE::IDENTIFIER,
        ADD_ROUTE::IDENTIFIER,
        REMOVE_ROUTE::IDENTIFIER,
        GET_DNS_SERVER_COUNT::IDENTIFIER,
        GET_DNS_SERVER::IDENTIFIER,
        ADD_DNS_SERVER::IDENTIFIER,
        REMOVE_DNS_SERVER::IDENTIFIER,
        SET_DHCP_STATE::IDENTIFIER,
        GET_DHCP_STATE::IDENTIFIER,
        GET_IP_ADDRESS_COUNT::IDENTIFIER,
        GET_IP_ADDRESS::IDENTIFIER,
        ADD_IP_ADDRESS::IDENTIFIER,
        REMOVE_IP_ADDRESS::IDENTIFIER,
        GET_WIFI_CAPABILITIES::IDENTIFIER,
        GET_WIFI_RADIO_STATE::IDENTIFIER,
        SET_WIFI_RADIO_STATE::IDENTIFIER,
        START_WIFI_SCAN::IDENTIFIER,
        GET_WIFI_SCAN_STATE::IDENTIFIER,
        GET_WIFI_SCAN_RESULT_COUNT::IDENTIFIER,
        GET_WIFI_SCAN_RESULT::IDENTIFIER,
        CONNECT_WIFI_CLIENT::IDENTIFIER,
        DISCONNECT_WIFI_CLIENT::IDENTIFIER,
        GET_WIFI_CONNECTION_STATE::IDENTIFIER,
        GET_WIFI_CONNECTION_INFO::IDENTIFIER,
        GET_WIFI_LAST_CONNECTION_ERROR::IDENTIFIER,
    ];
    for (i, id) in identifiers.iter().enumerate() {
        assert!(!identifiers[..i].contains(id), "duplicate {id:?}");
    }
}

#[test]
fn clearing_credentials_erases_owned_bytes_and_length() {
    let mut credential = WifiPassword::new(b"private-key").unwrap();
    assert!(!format!("{credential:?}").contains("private-key"));
    credential.clear();
    assert_eq!(credential.length, 0);
    assert_eq!(credential.bytes, [0; 64]);
}
