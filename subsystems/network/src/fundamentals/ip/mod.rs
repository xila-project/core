mod ipv4;
mod ipv6;

use core::fmt::Display;

pub use ipv4::*;
pub use ipv6::*;

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IpAddress {
    IPv4(Ipv4),
    IPv6(Ipv6),
}

impl Default for IpAddress {
    fn default() -> Self {
        IpAddress::IPv4(Ipv4::default())
    }
}

impl IpAddress {
    pub const fn new_ipv4(value: [u8; 4]) -> Self {
        Self::IPv4(Ipv4::new(value))
    }

    pub const fn new_ipv6(value: [u16; 8]) -> Self {
        Self::IPv6(Ipv6::new(value))
    }

    pub const fn into_smoltcp(&self) -> smoltcp::wire::IpAddress {
        match self {
            IpAddress::IPv4(value) => smoltcp::wire::IpAddress::Ipv4(value.into_smoltcp()),
            IpAddress::IPv6(value) => smoltcp::wire::IpAddress::Ipv6(value.into_smoltcp()),
        }
    }

    pub const fn from_smoltcp(value: &smoltcp::wire::IpAddress) -> Self {
        match value {
            smoltcp::wire::IpAddress::Ipv4(v4_addr) => {
                IpAddress::IPv4(crate::Ipv4::from_smoltcp(v4_addr))
            }
            smoltcp::wire::IpAddress::Ipv6(v6_addr) => {
                IpAddress::IPv6(crate::Ipv6::from_smoltcp(v6_addr))
            }
        }
    }
}

impl TryFrom<&str> for IpAddress {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Ok(ipv4) = Ipv4::try_from(value) {
            Ok(IpAddress::IPv4(ipv4))
        } else if let Ok(ipv6) = Ipv6::try_from(value) {
            Ok(IpAddress::IPv6(ipv6))
        } else {
            Err(())
        }
    }
}

impl From<[u8; 4]> for IpAddress {
    fn from(value: [u8; 4]) -> Self {
        IpAddress::IPv4(Ipv4::new(value))
    }
}

impl From<&[u8; 4]> for IpAddress {
    fn from(value: &[u8; 4]) -> Self {
        IpAddress::IPv4(Ipv4::new(*value))
    }
}

impl From<[u8; 16]> for IpAddress {
    fn from(value: [u8; 16]) -> Self {
        IpAddress::IPv6(Ipv6::from_inner(value))
    }
}

impl From<&[u8; 16]> for IpAddress {
    fn from(value: &[u8; 16]) -> Self {
        IpAddress::IPv6(Ipv6::from_inner(*value))
    }
}

impl From<IpAddress> for smoltcp::wire::IpAddress {
    fn from(value: IpAddress) -> Self {
        value.into_smoltcp()
    }
}

impl From<smoltcp::wire::IpAddress> for IpAddress {
    fn from(value: smoltcp::wire::IpAddress) -> Self {
        IpAddress::from_smoltcp(&value)
    }
}

impl Display for IpAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IpAddress::IPv4(value) => write!(f, "{value}"),
            IpAddress::IPv6(value) => write!(f, "{value}"),
        }
    }
}

impl From<Ipv4> for IpAddress {
    fn from(value: Ipv4) -> Self {
        Self::IPv4(value)
    }
}

impl From<Ipv6> for IpAddress {
    fn from(value: Ipv6) -> Self {
        Self::IPv6(value)
    }
}
