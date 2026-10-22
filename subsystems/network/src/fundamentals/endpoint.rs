use smoltcp::wire;

use crate::{IpAddress, Port};

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IpEndpoint {
    pub address: IpAddress,
    pub port: Port,
}

impl IpEndpoint {
    pub const fn new(address: IpAddress, port: Port) -> Self {
        Self { address, port }
    }

    pub const fn into_smoltcp(&self) -> wire::IpEndpoint {
        wire::IpEndpoint {
            addr: self.address.into_smoltcp(),
            port: self.port.into_inner(),
        }
    }

    pub const fn from_smoltcp(value: &wire::IpEndpoint) -> Self {
        Self {
            address: IpAddress::from_smoltcp(&value.addr),
            port: Port::from_inner(value.port),
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IpListenEndpoint {
    pub address: Option<IpAddress>,
    pub port: Port,
}

impl IpListenEndpoint {
    pub const fn new(address: Option<IpAddress>, port: Port) -> Self {
        Self { address, port }
    }

    pub const fn into_smoltcp(&self) -> wire::IpListenEndpoint {
        let smoltcp_address = match &self.address {
            Some(ip) => Some(ip.into_smoltcp()),
            None => None,
        };

        wire::IpListenEndpoint {
            addr: smoltcp_address,
            port: self.port.into_inner(),
        }
    }

    pub const fn from_smoltcp(value: &wire::IpListenEndpoint) -> Self {
        let address = match &value.addr {
            Some(ip) => Some(IpAddress::from_smoltcp(ip)),
            None => None,
        };

        Self {
            address,
            port: Port::from_inner(value.port),
        }
    }
}
