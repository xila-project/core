use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u8 {
        const Executable = 1 << 0;
        const DirectMemoryAccess = 1 << 1;
    }
}
