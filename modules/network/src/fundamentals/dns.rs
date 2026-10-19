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
