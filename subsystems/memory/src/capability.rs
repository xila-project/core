use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u8 {
        /// Memory can be used for code execution.
        const Executable = 1 << 0;
        /// Memory can be read and written by DMA controllers (subject to platform alignment constraints).
        const DirectMemoryAccess = 1 << 1;
        /// Memory remains CPU-accessible while external flash caches are disabled.
        const AlwaysAccessible = 1 << 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_flag_bit_values() {
        assert_eq!(CapabilityFlags::Executable.bits(), 1);
        assert_eq!(CapabilityFlags::DirectMemoryAccess.bits(), 2);
        assert_eq!(CapabilityFlags::AlwaysAccessible.bits(), 4);
        assert_eq!(CapabilityFlags::empty().bits(), 0);
        assert_eq!(CapabilityFlags::all().bits(), 0b111);
        assert!(CapabilityFlags::all().contains(CapabilityFlags::Executable));
        assert!(CapabilityFlags::all().contains(CapabilityFlags::DirectMemoryAccess));
        assert!(CapabilityFlags::all().contains(CapabilityFlags::AlwaysAccessible));
    }

    #[test]
    fn capability_flag_unknown_bits() {
        assert_eq!(CapabilityFlags::from_bits(1 << 3), None);
        assert_eq!(
            CapabilityFlags::from_bits_truncate(0b1111),
            CapabilityFlags::all()
        );
        assert_eq!(CapabilityFlags::from_bits_retain(1 << 3).bits(), 1 << 3);
    }
}
