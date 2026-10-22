use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CapabilityFlags: u8 {
        const Executable = 1 << 0;
        const DirectMemoryAccess = 1 << 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_flag_bit_values() {
        assert_eq!(CapabilityFlags::Executable.bits(), 1);
        assert_eq!(CapabilityFlags::DirectMemoryAccess.bits(), 2);
        assert_eq!(CapabilityFlags::empty().bits(), 0);
        assert_eq!(CapabilityFlags::all().bits(), 0b11);
        assert!(CapabilityFlags::all().contains(CapabilityFlags::Executable));
        assert!(CapabilityFlags::all().contains(CapabilityFlags::DirectMemoryAccess));
    }

    #[test]
    fn capability_flag_unknown_bits() {
        assert_eq!(CapabilityFlags::from_bits(1 << 2), None);
        assert_eq!(
            CapabilityFlags::from_bits_truncate(0b111),
            CapabilityFlags::all()
        );
        assert_eq!(CapabilityFlags::from_bits_retain(1 << 2).bits(), 1 << 2);
    }
}
