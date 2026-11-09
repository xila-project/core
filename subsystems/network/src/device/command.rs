use crate::{InterfaceKind, IpAddress, IpCidr, MacAddress, Route};
use file_system::{ControlCommand, define_command};

// GET_KIND previously collided with GET_STATE at n/1. Consumers must rebuild
// for n/21; keep all existing generic command numbers stable.
define_command!(
    GET_KIND,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetKind as u8,
    (),
    InterfaceKind
);

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
    GetKind,
}

define_command!(
    SET_STATE,
    Write,
    device::kind::NETWORK,
    CommandNumber::SetState as u8,
    bool,
    ()
);
define_command!(
    GET_STATE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetState as u8,
    (),
    bool
);
define_command!(
    GET_HARDWARE_ADDRESS,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetHardwareAddress as u8,
    (),
    MacAddress
);
define_command!(
    SET_HARDWARE_ADDRESS,
    Write,
    device::kind::NETWORK,
    CommandNumber::SetHardwareAddress as u8,
    MacAddress,
    ()
);
define_command!(
    GET_MAXIMUM_TRANSMISSION_UNIT,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetMtu as u8,
    (),
    usize
);
define_command!(
    GET_MAXIMUM_BURST_SIZE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetMaximumBurstSize as u8,
    (),
    Option<usize>
);
define_command!(
    IS_LINK_UP,
    Read,
    device::kind::NETWORK,
    CommandNumber::IsLinkUp as u8,
    (),
    bool
);
define_command!(
    GET_ROUTE_COUNT,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetRouteCount as u8,
    (),
    usize
);
define_command!(
    GET_ROUTE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetRoute as u8,
    usize,
    Route
);
define_command!(
    ADD_ROUTE,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddRoute as u8,
    Route,
    ()
);
define_command!(
    REMOVE_ROUTE,
    Write,
    device::kind::NETWORK,
    CommandNumber::RemoveRoute as u8,
    usize,
    ()
);
define_command!(
    GET_DNS_SERVER_COUNT,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetDnsServerCount as u8,
    (),
    usize
);
define_command!(
    GET_DNS_SERVER,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetDnsServer as u8,
    usize,
    IpAddress
);
define_command!(
    ADD_DNS_SERVER,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddDnsServer as u8,
    IpAddress,
    ()
);
define_command!(
    REMOVE_DNS_SERVER,
    Write,
    device::kind::NETWORK,
    CommandNumber::RemoveDnsServer as u8,
    usize,
    ()
);
define_command!(
    SET_DHCP_STATE,
    Write,
    device::kind::NETWORK,
    CommandNumber::SetDhcpState as u8,
    bool,
    ()
);
define_command!(
    GET_DHCP_STATE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetDhcpState as u8,
    (),
    bool
);
define_command!(
    GET_IP_ADDRESS_COUNT,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetIpAddressCount as u8,
    (),
    usize
);
define_command!(
    GET_IP_ADDRESS,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetIpAddress as u8,
    usize,
    IpCidr
);
define_command!(
    ADD_IP_ADDRESS,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddIpAddress as u8,
    IpCidr,
    ()
);
define_command!(
    REMOVE_IP_ADDRESS,
    Write,
    device::kind::NETWORK,
    CommandNumber::RemoveIpAddress as u8,
    usize,
    ()
);
