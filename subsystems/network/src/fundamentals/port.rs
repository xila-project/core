#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Port(u16);

impl Port {
    pub const MINIMUM_USER: Self = Self(1025);
    pub const MAXIMUM: Self = Self(u16::MAX);

    pub const DHCP_SERVER: Self = Self(67);
    pub const DHCP_CLIENT: Self = Self(68);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn into_inner(self) -> u16 {
        self.0
    }

    pub const fn from_inner(value: u16) -> Self {
        Self(value)
    }
}

impl From<u16> for Port {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

impl From<Port> for u16 {
    fn from(value: Port) -> Self {
        value.into_inner()
    }
}
