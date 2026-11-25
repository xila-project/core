use core::fmt::{Debug, Display};

use file_system::IoErrorKind;

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

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload mirror of [`IpCidr`]: same size and layout as the
    /// `#[repr(C)]` enum (a `u32` tag, the address bytes, then the prefix
    /// length), but every byte pattern is a valid value. `family` is `0` for
    /// IPv4 and `1` for IPv6; any other value is rejected with
    /// `InvalidArgument`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct IpCidrAbi {
        pub family: u32,
        pub address: [u8; 16],
        pub prefix_length: u8,
    }
}

impl From<IpCidr> for IpCidrAbi {
    fn from(value: IpCidr) -> Self {
        let (family, address, prefix_length) = match value {
            IpCidr::IPv4(cidr) => {
                let mut bytes = [0; 16];
                bytes[..4].copy_from_slice(&cidr.address.into_inner());
                (0, bytes, cidr.prefix_length)
            }
            IpCidr::IPv6(cidr) => (1, cidr.address.into_inner(), cidr.prefix_length),
        };
        Self {
            family,
            address,
            prefix_length,
        }
    }
}

impl TryFrom<IpCidrAbi> for IpCidr {
    type Error = IoErrorKind;

    fn try_from(value: IpCidrAbi) -> file_system::Result<Self> {
        match value.family {
            0 => {
                if value.prefix_length > 32 {
                    return Err(IoErrorKind::InvalidArgument);
                }
                let mut address = [0; 4];
                address.copy_from_slice(&value.address[..4]);
                Ok(IpCidr::IPv4(Cidr::new(
                    Ipv4::from_inner(address),
                    value.prefix_length,
                )))
            }
            1 => {
                if value.prefix_length > 128 {
                    return Err(IoErrorKind::InvalidArgument);
                }
                Ok(IpCidr::IPv6(Cidr::new(
                    Ipv6::from_inner(value.address),
                    value.prefix_length,
                )))
            }
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
}

#[cfg(test)]
mod abi_tests {
    use super::*;

    #[test]
    fn ip_cidr_abi_round_trips_and_keeps_the_enum_layout() {
        assert_eq!(
            core::mem::size_of::<IpCidrAbi>(),
            core::mem::size_of::<IpCidr>()
        );
        assert_eq!(
            core::mem::align_of::<IpCidrAbi>(),
            core::mem::align_of::<IpCidr>()
        );
        for cidr in [
            IpCidr::new_ipv4([10, 0, 0, 1], 24),
            IpCidr::new_ipv6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0], 64),
        ] {
            assert_eq!(
                IpCidr::try_from(IpCidrAbi::from(cidr.clone())).unwrap(),
                cidr
            );
        }
    }

    #[test]
    fn ip_cidr_abi_rejects_unknown_families_and_prefixes() {
        let unknown_family = IpCidrAbi {
            family: 5,
            address: [0; 16],
            prefix_length: 24,
        };
        assert_eq!(
            IpCidr::try_from(unknown_family),
            Err(IoErrorKind::InvalidArgument)
        );
        let ipv4_too_long = IpCidrAbi {
            family: 0,
            address: [0; 16],
            prefix_length: 33,
        };
        assert_eq!(
            IpCidr::try_from(ipv4_too_long),
            Err(IoErrorKind::InvalidArgument)
        );
        let ipv6_too_long = IpCidrAbi {
            family: 1,
            address: [0; 16],
            prefix_length: 129,
        };
        assert_eq!(
            IpCidr::try_from(ipv6_too_long),
            Err(IoErrorKind::InvalidArgument)
        );
    }
}
