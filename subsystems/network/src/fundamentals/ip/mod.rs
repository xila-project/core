mod ipv4;
mod ipv6;

use core::fmt::Display;

use file_system::IoErrorKind;

pub use ipv4::*;
pub use ipv6::*;

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IpAddress {
    IPv4(Ipv4),
    IPv6(Ipv6),
}

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload mirror of [`IpAddress`]: same size (20 bytes) and
    /// alignment (4) as the `#[repr(C)]` enum, but every byte pattern is a
    /// valid value. Layout: `family` at offset 0 (a `u32` tag), `address` at
    /// offset 4 (16 bytes). `family` is `0` for IPv4 and `1` for IPv6; any other
    /// value is rejected with `InvalidArgument`, as are non-zero bytes in the
    /// trailing 12 address bytes of an IPv4 address.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
    pub struct IpAddressAbi {
        pub family: u32,
        pub address: [u8; 16],
    }
}

impl From<IpAddress> for IpAddressAbi {
    fn from(value: IpAddress) -> Self {
        match value {
            IpAddress::IPv4(address) => {
                let mut bytes = [0; 16];
                bytes[..4].copy_from_slice(&address.into_inner());
                Self {
                    family: 0,
                    address: bytes,
                }
            }
            IpAddress::IPv6(address) => Self {
                family: 1,
                address: address.into_inner(),
            },
        }
    }
}

impl TryFrom<IpAddressAbi> for IpAddress {
    type Error = IoErrorKind;

    fn try_from(value: IpAddressAbi) -> file_system::Result<Self> {
        match value.family {
            0 => {
                // An IPv4 address uses only the first 4 address bytes; the
                // trailing 12 must be zero.
                if value.address[4..].iter().any(|byte| *byte != 0) {
                    return Err(IoErrorKind::InvalidArgument);
                }
                Ok(IpAddress::IPv4(Ipv4::from_inner(
                    value.address[..4]
                        .try_into()
                        .map_err(|_| IoErrorKind::InvalidArgument)?,
                )))
            }
            1 => Ok(IpAddress::IPv6(Ipv6::from_inner(value.address))),
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
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

#[cfg(test)]
mod abi_tests {
    use super::*;

    #[test]
    fn ip_address_abi_round_trips_and_keeps_the_enum_layout() {
        assert_eq!(
            core::mem::size_of::<IpAddressAbi>(),
            core::mem::size_of::<IpAddress>()
        );
        assert_eq!(
            core::mem::align_of::<IpAddressAbi>(),
            core::mem::align_of::<IpAddress>()
        );
        for address in [
            IpAddress::new_ipv4([192, 168, 0, 1]),
            IpAddress::new_ipv6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        ] {
            assert_eq!(
                IpAddress::try_from(IpAddressAbi::from(address.clone())).unwrap(),
                address
            );
        }
    }

    #[test]
    fn ip_address_abi_rejects_an_unknown_family() {
        let abi = IpAddressAbi {
            family: 2,
            address: [0; 16],
        };
        assert_eq!(IpAddress::try_from(abi), Err(IoErrorKind::InvalidArgument));
    }

    #[test]
    fn ip_address_abi_rejects_non_zero_ipv4_trailing_bytes() {
        let mut abi = IpAddressAbi::from(IpAddress::new_ipv4([192, 168, 0, 1]));
        assert_eq!(
            core::mem::size_of::<IpAddressAbi>(),
            core::mem::size_of::<u32>() + core::mem::size_of::<[u8; 16]>(),
            "IpAddressAbi must be padding-free"
        );
        abi.address[15] = 1;
        assert_eq!(IpAddress::try_from(abi), Err(IoErrorKind::InvalidArgument));
    }
}
