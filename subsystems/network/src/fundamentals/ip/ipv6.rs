use core::fmt::Display;
use core::str::FromStr;

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
        // `core::net::Ipv6Addr` accepts every RFC 4291 textual form:
        // full, compressed (`2001:db8::1`, `::1`, `::`) and IPv4-mapped
        // (`::ffff:192.0.2.1`).
        let address = core::net::Ipv6Addr::from_str(value).map_err(|_| ())?;
        Ok(Self(address.octets()))
    }
}

impl Display for Ipv6 {
    /// Writes the canonical RFC 5952 form (lowercase hexadecimal, longest
    /// run of zero groups compressed, first run on a tie), which
    /// [`Ipv6::try_from`] reads back.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Display::fmt(&self.into_smoltcp(), f)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_ipv6_display() {
        let ip = Ipv6::new([0; 8]);

        assert_eq!(ip.to_string(), "::");
    }

    #[test]
    fn display_compresses_the_longest_zero_run() {
        let ip = Ipv6::new([0x2606, 0x4700, 0x10, 0, 0, 0, 0x6814, 0x179a]);

        assert_eq!(ip.to_string(), "2606:4700:10::6814:179a");
        assert_eq!(Ipv6::try_from(ip.to_string().as_str()).unwrap(), ip);
    }

    #[test]
    fn display_compresses_the_first_run_on_a_tie() {
        let ip = Ipv6::new([0, 0, 1, 0, 0, 2, 3, 4]);

        assert_eq!(ip.to_string(), "::1:0:0:2:3:4");
    }

    #[test]
    fn display_uses_lowercase_hexadecimal() {
        let ip = Ipv6::new([0xabcd, 0xef01, 0, 0, 0, 0, 0, 1]);

        assert_eq!(ip.to_string(), "abcd:ef01::1");
    }

    #[test]
    fn display_prints_ipv4_mapped_addresses_in_mixed_notation() {
        let ip = Ipv6::try_from("::ffff:192.0.2.1").unwrap();

        assert_eq!(ip.to_string(), "::ffff:192.0.2.1");
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

    #[test]
    fn test_ipv6_try_from_compressed_form() {
        let ip = Ipv6::try_from("2001:db8::1").unwrap();

        assert_eq!(ip, Ipv6::new([0x2001, 0x0db8, 0, 0, 0, 0, 0, 1]));

        assert_eq!(
            Ipv6::try_from("::1").unwrap(),
            Ipv6::new([0, 0, 0, 0, 0, 0, 0, 1])
        );

        assert_eq!(Ipv6::try_from("::").unwrap(), Ipv6::new([0; 8]));

        assert_eq!(
            Ipv6::try_from("2001:db8::1:0:0:1").unwrap(),
            Ipv6::try_from("2001:db8:0:0:1:0:0:1").unwrap()
        );
    }

    #[test]
    fn test_ipv6_try_from_ipv4_mapped() {
        let ip = Ipv6::try_from("::ffff:192.0.2.1").unwrap();

        assert_eq!(ip, Ipv6::new([0, 0, 0, 0, 0, 0xffff, 0xc000, 0x0201]));
        assert_eq!(Ipv6::try_from(ip.to_string().as_str()).unwrap(), ip);
    }

    #[test]
    fn test_ipv6_try_from_invalid() {
        for input in [
            "",
            "0:0:0:0:0:0:0:0:0",
            "0:0:0:0:0:0:0",
            "::1::",
            "2001:db8:::1",
            "12345::",
            "gg::1",
            "192.0.2.1",
            "-1::",
            " 2001:db8::1",
        ] {
            Ipv6::try_from(input).unwrap_err();
        }
    }

    #[test]
    fn display_round_trips_through_try_from() {
        for input in [
            "::",
            "::1",
            "2001:db8::1",
            "2606:4700:10::6814:179a",
            "::ffff:192.0.2.1",
            "1234:5678:9abc:def0:1234:5678:9abc:def0",
        ] {
            let ip = Ipv6::try_from(input).unwrap();

            assert_eq!(Ipv6::try_from(ip.to_string().as_str()).unwrap(), ip);
        }
    }
}
