use smoltcp::{phy::PacketMeta, socket::udp, wire};

use crate::{IpAddress, Port};

#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UdpMetadata {
    pub remote_address: IpAddress,
    pub local_address: Option<IpAddress>,
    pub remote_port: Port,
    pub meta: PacketMeta,
}

impl UdpMetadata {
    pub fn new(
        remote_address: impl Into<IpAddress>,
        remote_port: impl Into<Port>,
        local_address: Option<IpAddress>,
        meta: PacketMeta,
    ) -> Self {
        Self {
            remote_address: remote_address.into(),
            local_address,
            remote_port: remote_port.into(),
            meta,
        }
    }

    pub const fn from_smoltcp(value: &udp::UdpMetadata) -> Self {
        let local_address = match value.local_address {
            Some(addr) => Some(IpAddress::from_smoltcp(&addr)),
            None => None,
        };

        Self {
            remote_address: IpAddress::from_smoltcp(&value.endpoint.addr),
            local_address,
            remote_port: Port::from_inner(value.endpoint.port),
            meta: value.meta,
        }
    }

    pub const fn to_smoltcp(&self) -> udp::UdpMetadata {
        let local_address = match &self.local_address {
            Some(addr) => Some(addr.into_smoltcp()),
            None => None,
        };

        udp::UdpMetadata {
            endpoint: wire::IpEndpoint::new(
                self.remote_address.into_smoltcp(),
                self.remote_port.into_inner(),
            ),
            local_address,
            meta: self.meta,
        }
    }
}
