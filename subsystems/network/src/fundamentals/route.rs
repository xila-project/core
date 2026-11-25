use core::time::Duration;

use file_system::IoErrorKind;

use crate::{IpAddress, IpAddressAbi, IpCidr, IpCidrAbi};

#[repr(C)]
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Route {
    pub cidr: IpCidr,
    pub via_router: IpAddress,
    /// Absolute monotonic time in the time manager source's epoch, not UTC.
    pub preferred_until: Option<Duration>,
    /// Absolute monotonic time in the time manager source's epoch, not UTC.
    pub expires_at: Option<Duration>,
}

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload mirror of an optional [`Duration`]: same size and
    /// alignment as `Option<Duration>`, but every byte pattern is a valid
    /// value. `present` is `0` or `1`; any other value is rejected with
    /// `InvalidArgument`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct OptionalDurationAbi {
        pub seconds: u64,
        pub nanos: u32,
        pub present: u8,
        pub reserved: [u8; 3],
    }
}

impl From<Option<Duration>> for OptionalDurationAbi {
    fn from(value: Option<Duration>) -> Self {
        match value {
            Some(duration) => Self {
                seconds: duration.as_secs(),
                nanos: duration.subsec_nanos(),
                present: 1,
                reserved: [0; 3],
            },
            None => Self::default(),
        }
    }
}

impl TryFrom<OptionalDurationAbi> for Option<Duration> {
    type Error = IoErrorKind;

    fn try_from(value: OptionalDurationAbi) -> file_system::Result<Self> {
        match value.present {
            0 if value.seconds == 0 && value.nanos == 0 && value.reserved == [0; 3] => Ok(None),
            1 => {
                if value.nanos >= 1_000_000_000 || value.reserved != [0; 3] {
                    return Err(IoErrorKind::InvalidArgument);
                }
                Ok(Some(Duration::new(value.seconds, value.nanos)))
            }
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
}

file_system::plain_data! {
    #[repr(C)]
    /// Plain-data payload mirror of [`Route`]: same size and layout, but every
    /// byte pattern is a valid value. Unknown families and invalid optional
    /// durations are rejected with `InvalidArgument` when converting back.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct RouteAbi {
        pub cidr: IpCidrAbi,
        pub via_router: IpAddressAbi,
        pub preferred_until: OptionalDurationAbi,
        pub expires_at: OptionalDurationAbi,
    }
}

impl From<Route> for RouteAbi {
    fn from(value: Route) -> Self {
        Self {
            cidr: value.cidr.into(),
            via_router: value.via_router.into(),
            preferred_until: value.preferred_until.into(),
            expires_at: value.expires_at.into(),
        }
    }
}

impl TryFrom<RouteAbi> for Route {
    type Error = IoErrorKind;

    fn try_from(value: RouteAbi) -> file_system::Result<Self> {
        Ok(Self {
            cidr: value.cidr.try_into()?,
            via_router: value.via_router.try_into()?,
            preferred_until: value.preferred_until.try_into()?,
            expires_at: value.expires_at.try_into()?,
        })
    }
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
            Some(Duration::from_micros(dur.total_micros() as u64))
        } else {
            None
        };

        let expires_at = if let Some(dur) = route.expires_at {
            Some(Duration::from_micros(dur.total_micros() as u64))
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

#[cfg(test)]
mod tests {
    use super::Route;
    use core::time::Duration;

    #[test]
    fn smoltcp_conversion_keeps_whole_seconds() {
        let mut route = Route::new_default_ipv4([192, 0, 2, 1]);
        route.preferred_until = Some(Duration::from_millis(30_500));
        route.expires_at = Some(Duration::from_secs(60));

        assert!(Route::from_smoltcp(route.into_smoltcp()) == route);
    }
}

#[cfg(test)]
mod abi_tests {
    use super::*;

    #[test]
    fn route_abi_round_trips_and_keeps_the_layout() {
        assert_eq!(
            core::mem::size_of::<RouteAbi>(),
            core::mem::size_of::<Route>()
        );
        assert_eq!(
            core::mem::align_of::<RouteAbi>(),
            core::mem::align_of::<Route>()
        );
        let route = Route {
            cidr: IpCidr::new_ipv4([192, 168, 0, 0], 24),
            via_router: IpAddress::new_ipv4([192, 168, 0, 1]),
            preferred_until: Some(Duration::from_millis(1500)),
            expires_at: None,
        };
        assert!(Route::try_from(RouteAbi::from(route.clone())).unwrap() == route);
    }

    #[test]
    fn optional_duration_abi_rejects_unknown_values() {
        assert_eq!(
            Option::<Duration>::try_from(OptionalDurationAbi {
                seconds: 1,
                nanos: 0,
                present: 2,
                reserved: [0; 3],
            }),
            Err(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            Option::<Duration>::try_from(OptionalDurationAbi {
                seconds: 1,
                nanos: 1_000_000_000,
                present: 1,
                reserved: [0; 3],
            }),
            Err(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            Option::<Duration>::try_from(OptionalDurationAbi {
                seconds: 3,
                nanos: 4,
                present: 0,
                reserved: [0; 3],
            }),
            Err(IoErrorKind::InvalidArgument)
        );
        assert_eq!(
            Option::<Duration>::try_from(OptionalDurationAbi {
                seconds: 3,
                nanos: 4,
                present: 1,
                reserved: [0; 3],
            })
            .unwrap(),
            Some(Duration::new(3, 4))
        );
    }

    #[test]
    fn route_abi_rejects_an_invalid_nested_value() {
        let mut route = RouteAbi::default();
        route.cidr.family = 8;
        assert_eq!(
            Route::try_from(route).err(),
            Some(IoErrorKind::InvalidArgument)
        );
    }
}
