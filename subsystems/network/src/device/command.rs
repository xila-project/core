use crate::{IpAddressAbi, IpCidrAbi, MacAddress, RouteAbi};
use file_system::{ControlCommand, IoErrorKind, define_command};

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload mirror of `Option<usize>`: same size and alignment
    /// on every target, but every byte pattern is a valid value. `present` is
    /// `0` or `1`; any other value is rejected with `InvalidArgument`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct OptionalUsizeAbi {
        pub present: u8,
        pub reserved: [u8; core::mem::size_of::<usize>() - 1],
        pub value: usize,
    }
}

impl From<Option<usize>> for OptionalUsizeAbi {
    fn from(value: Option<usize>) -> Self {
        match value {
            Some(value) => Self {
                present: 1,
                reserved: [0; core::mem::size_of::<usize>() - 1],
                value,
            },
            None => Self::default(),
        }
    }
}

impl TryFrom<OptionalUsizeAbi> for Option<usize> {
    type Error = IoErrorKind;

    fn try_from(value: OptionalUsizeAbi) -> file_system::Result<Self> {
        match value.present {
            0 if value.value == 0 => Ok(None),
            1 => Ok(Some(value.value)),
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
}

// GET_KIND previously collided with GET_STATE at n/1. Consumers must rebuild
// for n/21; keep all existing generic command numbers stable.
define_command!(
    GET_KIND,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetKind as u8,
    (),
    u8
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

// Payload convention: a payload must be plain data, so a payload byte pattern
// can never be an invalid value of the payload type. Flags and enum-like
// values are stored as bytes (`0` or `1`, or the enum discriminant) and
// converted with `TryFrom` where the value is read, rejecting unknown values
// with `InvalidArgument`. Structured values use their plain-data `*Abi`
// mirrors (`IpAddressAbi`, `IpCidrAbi`, `RouteAbi`, `OptionalUsizeAbi`).
define_command!(
    SET_STATE,
    Write,
    device::kind::NETWORK,
    CommandNumber::SetState as u8,
    u8,
    ()
);
define_command!(
    GET_STATE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetState as u8,
    (),
    u8
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
    OptionalUsizeAbi
);
define_command!(
    IS_LINK_UP,
    Read,
    device::kind::NETWORK,
    CommandNumber::IsLinkUp as u8,
    (),
    u8
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
    RouteAbi
);
define_command!(
    ADD_ROUTE,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddRoute as u8,
    RouteAbi,
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
    IpAddressAbi
);
define_command!(
    ADD_DNS_SERVER,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddDnsServer as u8,
    IpAddressAbi,
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
    u8,
    ()
);
define_command!(
    GET_DHCP_STATE,
    Read,
    device::kind::NETWORK,
    CommandNumber::GetDhcpState as u8,
    (),
    u8
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
    IpCidrAbi
);
define_command!(
    ADD_IP_ADDRESS,
    Write,
    device::kind::NETWORK,
    CommandNumber::AddIpAddress as u8,
    IpCidrAbi,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_usize_abi_round_trips_and_keeps_the_option_layout() {
        assert_eq!(
            core::mem::size_of::<OptionalUsizeAbi>(),
            core::mem::size_of::<Option<usize>>()
        );
        assert_eq!(
            core::mem::align_of::<OptionalUsizeAbi>(),
            core::mem::align_of::<Option<usize>>()
        );
        assert_eq!(
            Option::<usize>::try_from(OptionalUsizeAbi::from(Some(1500))).unwrap(),
            Some(1500)
        );
        assert_eq!(
            Option::<usize>::try_from(OptionalUsizeAbi::from(None)).unwrap(),
            None
        );
    }

    #[test]
    fn optional_usize_abi_rejects_an_unknown_present_byte() {
        let abi = OptionalUsizeAbi {
            present: 2,
            reserved: [0; core::mem::size_of::<usize>() - 1],
            value: 0,
        };
        assert_eq!(
            Option::<usize>::try_from(abi),
            Err(IoErrorKind::InvalidArgument)
        );
    }
}
