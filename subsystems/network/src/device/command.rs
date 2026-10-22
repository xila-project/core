use crate::{InterfaceKind, IpAddress, IpCidr, MacAddress, Route};
use file_system::{ControlCommand, define_command};

#[repr(C)]
pub struct WifiClientConfiguration {
    // TODO: Add fields
}

define_command!(GET_KIND, Read, b'n', 1, (), InterfaceKind);

#[repr(u8)]
enum CommandNumber {
    SetState,
    GetState,
    GetHardwareAddress,
    SetHardwareAddress,
    GetMtu,
    GetMaximumBurstSize,
    IsLinkUp,
    GetRouteCount,
    GetRoute,
    AddRoute,
    RemoveRoute,
    GetDnsServerCount,
    GetDnsServer,
    AddDnsServer,
    RemoveDnsServer,
    SetDhcpState,
    GetDhcpState,
    GetIpAddressCount,
    GetIpAddress,
    AddIpAddress,
    RemoveIpAddress,
}

define_command!(
    SET_STATE,
    Write,
    b'n',
    CommandNumber::SetState as u8,
    bool,
    ()
);
define_command!(
    GET_STATE,
    Read,
    b'n',
    CommandNumber::GetState as u8,
    (),
    bool
);
define_command!(
    GET_HARDWARE_ADDRESS,
    Read,
    b'n',
    CommandNumber::GetHardwareAddress as u8,
    (),
    MacAddress
);
define_command!(
    SET_HARDWARE_ADDRESS,
    Write,
    b'n',
    CommandNumber::SetHardwareAddress as u8,
    MacAddress,
    ()
);
define_command!(
    GET_MAXIMUM_TRANSMISSION_UNIT,
    Read,
    b'n',
    CommandNumber::GetMtu as u8,
    (),
    usize
);
define_command!(
    GET_MAXIMUM_BURST_SIZE,
    Read,
    b'n',
    CommandNumber::GetMaximumBurstSize as u8,
    (),
    Option<usize>
);
define_command!(
    IS_LINK_UP,
    Read,
    b'n',
    CommandNumber::IsLinkUp as u8,
    (),
    bool
);
define_command!(
    GET_ROUTE_COUNT,
    Read,
    b'n',
    CommandNumber::GetRouteCount as u8,
    (),
    usize
);
define_command!(
    GET_ROUTE,
    Read,
    b'n',
    CommandNumber::GetRoute as u8,
    usize,
    Route
);
define_command!(
    ADD_ROUTE,
    Write,
    b'n',
    CommandNumber::AddRoute as u8,
    Route,
    ()
);
define_command!(
    REMOVE_ROUTE,
    Write,
    b'n',
    CommandNumber::RemoveRoute as u8,
    usize,
    ()
);
define_command!(
    GET_DNS_SERVER_COUNT,
    Read,
    b'n',
    CommandNumber::GetDnsServerCount as u8,
    (),
    usize
);
define_command!(
    GET_DNS_SERVER,
    Read,
    b'n',
    CommandNumber::GetDnsServer as u8,
    usize,
    IpAddress
);
define_command!(
    ADD_DNS_SERVER,
    Write,
    b'n',
    CommandNumber::AddDnsServer as u8,
    IpAddress,
    ()
);
define_command!(
    REMOVE_DNS_SERVER,
    Write,
    b'n',
    CommandNumber::RemoveDnsServer as u8,
    usize,
    ()
);
define_command!(
    SET_DHCP_STATE,
    Write,
    b'n',
    CommandNumber::SetDhcpState as u8,
    bool,
    ()
);
define_command!(
    GET_DHCP_STATE,
    Read,
    b'n',
    CommandNumber::GetDhcpState as u8,
    (),
    bool
);
define_command!(
    GET_IP_ADDRESS_COUNT,
    Read,
    b'n',
    CommandNumber::GetIpAddressCount as u8,
    (),
    usize
);
define_command!(
    GET_IP_ADDRESS,
    Read,
    b'n',
    CommandNumber::GetIpAddress as u8,
    usize,
    IpCidr
);
define_command!(
    ADD_IP_ADDRESS,
    Write,
    b'n',
    CommandNumber::AddIpAddress as u8,
    IpCidr,
    ()
);
define_command!(
    REMOVE_IP_ADDRESS,
    Write,
    b'n',
    CommandNumber::RemoveIpAddress as u8,
    usize,
    ()
);
