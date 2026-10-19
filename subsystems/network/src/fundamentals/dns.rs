use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DnsQueryKind: u8 {
        const A = 1 << 0;
        const Aaaa = 1 << 1;
        const Cname = 1 << 2;
        const Ns = 1 << 3;
        const Soa = 1 << 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_query_kind_bit_values() {
        assert_eq!(DnsQueryKind::A.bits(), 1 << 0);
        assert_eq!(DnsQueryKind::Aaaa.bits(), 1 << 1);
        assert_eq!(DnsQueryKind::Cname.bits(), 1 << 2);
        assert_eq!(DnsQueryKind::Ns.bits(), 1 << 3);
        assert_eq!(DnsQueryKind::Soa.bits(), 1 << 4);
        assert_eq!(DnsQueryKind::empty().bits(), 0);
        assert_eq!(DnsQueryKind::all().bits(), 0b1_1111);
    }

    #[test]
    fn dns_query_kind_unknown_bits() {
        assert_eq!(DnsQueryKind::from_bits(1 << 5), None);
        assert_eq!(
            DnsQueryKind::from_bits_truncate(0b11_1111),
            DnsQueryKind::all()
        );
        assert_eq!(DnsQueryKind::from_bits_retain(1 << 5).bits(), 1 << 5);
    }
}
