use core::fmt::Display;

use crate::Ipv6;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Ipv4([u8; 4]);

impl Ipv4 {
    pub const LOCALHOST: Self = Self([127, 0, 0, 1]);
    pub const BROADCAST: Self = Self([255, 255, 255, 255]);

    pub const fn new(value: [u8; 4]) -> Self {
        Self(value)
    }

    pub const fn into_inner(self) -> [u8; 4] {
        self.0
    }

    pub const fn from_inner(value: [u8; 4]) -> Self {
        Self(value)
    }

    pub const fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }

    pub const fn is_broadcast(&self) -> bool {
        u32::from_be_bytes(self.0) == u32::from_be_bytes(Self::BROADCAST.0)
    }

    pub const fn to_ipv6_mapped(&self) -> Ipv6 {
        Ipv6::new([
            0,
            0,
            0,
            0,
            0,
            0xFFFF,
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
        ])
    }

    pub const fn into_smoltcp(self) -> core::net::Ipv4Addr {
        core::net::Ipv4Addr::new(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub const fn from_smoltcp(value: &core::net::Ipv4Addr) -> Self {
        Self(value.octets())
    }
}

impl TryFrom<&str> for Ipv4 {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut result = [0; 4];
        let mut index = 0;

        for part in value.split('.') {
            if index >= 4 {
                return Err(());
            }
            let part = part.parse::<u8>().map_err(|_| ())?;
            result[index] = part;
            index += 1;
        }
        if index != 4 {
            return Err(());
        }

        Ok(Self::new(result))
    }
}

impl Display for Ipv4 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_ipv4_display() {
        let ip = Ipv4::new([192, 168, 1, 1]);

        assert_eq!(ip.to_string(), "192.168.1.1");
    }

    #[test]
    fn test_ipv4_try_from() {
        let ip = Ipv4::try_from("0.0.0.0").unwrap();

        assert_eq!(ip.0, [0, 0, 0, 0]);

        Ipv4::try_from("1.2b.3.4").unwrap_err();

        Ipv4::try_from("1.2.3.4.5").unwrap_err();

        Ipv4::try_from("1.2.3").unwrap_err();

        let ip = Ipv4::try_from("4.3.2.1").unwrap();

        assert_eq!(ip.0, [4, 3, 2, 1]);
    }
}
