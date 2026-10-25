use core::fmt::{Debug, Display};

use crate::{Ipv4, Ipv6};

#[repr(C)]
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Cidr<T: Default + Clone + PartialEq + Eq> {
    pub address: T,
    pub prefix_length: u8,
}

impl<T: Debug + Default + Clone + PartialEq + Eq> Cidr<T> {
    pub const fn new(address: T, prefix_length: u8) -> Self {
        Self {
            address,
            prefix_length,
        }
    }
}

impl<T: Debug + Default + Clone + PartialEq + Eq> Debug for Cidr<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}/{}", self.address, self.prefix_length)
    }
}

impl<T: Display + Default + Clone + PartialEq + Eq> Display for Cidr<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix_length)
    }
}

impl Cidr<Ipv4> {
    pub const fn into_smoltcp(&self) -> smoltcp::wire::Ipv4Cidr {
        smoltcp::wire::Ipv4Cidr::new(self.address.into_smoltcp(), self.prefix_length)
    }

    pub const fn from_smoltcp(value: &smoltcp::wire::Ipv4Cidr) -> Self {
        Self {
            address: Ipv4::from_smoltcp(&value.address()),
            prefix_length: value.prefix_len(),
        }
    }
}

impl Cidr<Ipv6> {
    pub const fn into_smoltcp(&self) -> smoltcp::wire::Ipv6Cidr {
        smoltcp::wire::Ipv6Cidr::new(self.address.into_smoltcp(), self.prefix_length)
    }

    pub const fn from_smoltcp(value: &smoltcp::wire::Ipv6Cidr) -> Self {
        Self {
            address: Ipv6::from_smoltcp(&value.address()),
            prefix_length: value.prefix_len(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpCidr {
    IPv4(Cidr<Ipv4>),
    IPv6(Cidr<Ipv6>),
}

impl Default for IpCidr {
    fn default() -> Self {
        IpCidr::IPv4(Cidr::default())
    }
}

impl IpCidr {
    pub const fn new_ipv4(address: [u8; 4], prefix_length: u8) -> Self {
        IpCidr::IPv4(Cidr::new(Ipv4::new(address), prefix_length))
    }

    pub const fn new_ipv6(address: [u16; 8], prefix_length: u8) -> Self {
        IpCidr::IPv6(Cidr::new(Ipv6::new(address), prefix_length))
    }

    pub const fn into_smoltcp(&self) -> smoltcp::wire::IpCidr {
        match self {
            IpCidr::IPv4(cidr) => smoltcp::wire::IpCidr::Ipv4(cidr.into_smoltcp()),
            IpCidr::IPv6(cidr) => smoltcp::wire::IpCidr::Ipv6(cidr.into_smoltcp()),
        }
    }

    pub const fn from_smoltcp(value: &smoltcp::wire::IpCidr) -> Self {
        match value {
            smoltcp::wire::IpCidr::Ipv4(cidr) => IpCidr::IPv4(Cidr::<Ipv4>::from_smoltcp(cidr)),
            smoltcp::wire::IpCidr::Ipv6(cidr) => IpCidr::IPv6(Cidr::<Ipv6>::from_smoltcp(cidr)),
        }
    }
}

impl Display for IpCidr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IpCidr::IPv4(cidr) => write!(f, "{cidr}"),
            IpCidr::IPv6(cidr) => write!(f, "{cidr}"),
        }
    }
}
