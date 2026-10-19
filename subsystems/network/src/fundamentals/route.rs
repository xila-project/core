use core::time::Duration;

use crate::{IpAddress, IpCidr};

#[repr(C)]
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Route {
    pub cidr: IpCidr,
    pub via_router: IpAddress,
    pub preferred_until: Option<Duration>,
    pub expires_at: Option<Duration>,
}

impl Route {
    pub const fn new_default_ipv4(via_router: [u8; 4]) -> Self {
        Self {
            cidr: IpCidr::new_ipv4([0, 0, 0, 0], 0),
            via_router: IpAddress::new_ipv4(via_router),
            preferred_until: None,
            expires_at: None,
        }
    }

    pub const fn new_default_ipv6(via_router: [u16; 8]) -> Self {
        Self {
            cidr: IpCidr::new_ipv6([0, 0, 0, 0, 0, 0, 0, 0], 0),
            via_router: IpAddress::new_ipv6(via_router),
            preferred_until: None,
            expires_at: None,
        }
    }

    pub const fn from_smoltcp(route: smoltcp::iface::Route) -> Self {
        let preferred_until = if let Some(dur) = route.preferred_until {
            Some(Duration::from_micros(dur.micros() as u64))
        } else {
            None
        };

        let expires_at = if let Some(dur) = route.expires_at {
            Some(Duration::from_micros(dur.micros() as u64))
        } else {
            None
        };

        Self {
            cidr: IpCidr::from_smoltcp(&route.cidr),
            via_router: IpAddress::from_smoltcp(&route.via_router),
            preferred_until,
            expires_at,
        }
    }

    pub fn into_smoltcp(&self) -> smoltcp::iface::Route {
        let preferred_until = self
            .preferred_until
            .map(|dur| smoltcp::time::Instant::from_micros(dur.as_micros() as i64));

        let expires_at = self
            .expires_at
            .map(|dur| smoltcp::time::Instant::from_micros(dur.as_micros() as i64));

        smoltcp::iface::Route {
            cidr: self.cidr.into_smoltcp(),
            via_router: self.via_router.into_smoltcp(),
            preferred_until,
            expires_at,
        }
    }
}
