use core::fmt::Display;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Ipv6([u8; 16]); // Avoid 2 byte alignment issues

impl Ipv6 {
    pub const fn new(value: [u16; 8]) -> Self {
        let mut bytes = [0; 16];
        let mut i = 0;
        while i < 8 {
            let segment = value[i].to_be_bytes();
            bytes[i * 2] = segment[0];
            bytes[i * 2 + 1] = segment[1];
            i += 1;
        }
        Self(bytes)
    }

    pub const fn into_inner(self) -> [u8; 16] {
        self.0
    }

    pub const fn from_inner(value: [u8; 16]) -> Self {
        Self(value)
    }

    pub const fn into_smoltcp(self) -> core::net::Ipv6Addr {
        core::net::Ipv6Addr::from_octets(self.0)
    }

    pub const fn from_smoltcp(value: &core::net::Ipv6Addr) -> Self {
        Self(value.octets())
    }
}

impl TryFrom<&str> for Ipv6 {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut result = [0; 8];
        let mut index = 0;

        for part in value.split(':') {
            if index >= 8 {
                return Err(());
            }
            let part = u16::from_str_radix(part, 16).map_err(|_| ())?;
            result[index] = part;
            index += 1;
        }

        if index != 8 {
            return Err(());
        }

        Ok(Self::new(result))
    }
}

impl Display for Ipv6 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:x}{:x}:{:x}{:x}:{:x}{:x}:{:x}{:x}:{:x}{:x}:{:x}{:x}:{:x}{:x}:{:x}{:x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3],
            self.0[4],
            self.0[5],
            self.0[6],
            self.0[7],
            self.0[8],
            self.0[9],
            self.0[10],
            self.0[11],
            self.0[12],
            self.0[13],
            self.0[14],
            self.0[15]
        )
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_ipv6_display() {
        let ip = Ipv6::new([0; 8]);

        assert_eq!(ip.to_string(), "00:00:00:00:00:00:00:00");
    }

    #[test]
    fn test_ipv6_try_from() {
        let ip = Ipv6::try_from("0:0:0:0:0:0:0:0").unwrap();

        assert_eq!(ip.0, [0; 16]);

        Ipv6::try_from("0:0:0:0:0:0:0:0:0").unwrap_err();

        Ipv6::try_from("0:0:0:0:0:0:0").unwrap_err();

        let ip = Ipv6::try_from("1234:5678:9abc:def0:1234:5678:9abc:def0").unwrap();

        assert_eq!(
            ip.0,
            [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
                0xde, 0xf0
            ]
        );
    }
}
