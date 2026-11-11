//! Partition type definitions for MBR partition tables.

/// Minimal MBR partition type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PartitionKind {
    /// Empty/unused partition slot (0x00).
    #[default]
    Empty,
    /// Xila filesystem partition (0xDA).
    Xila,
    /// Other partition type.
    Other(u8),
}

impl From<u8> for PartitionKind {
    fn from(val: u8) -> Self {
        match val {
            0x00 => Self::Empty,
            0xDA => Self::Xila,
            other => Self::Other(other),
        }
    }
}

impl From<PartitionKind> for u8 {
    fn from(kind: PartitionKind) -> Self {
        match kind {
            PartitionKind::Empty => 0x00,
            PartitionKind::Xila => 0xDA,
            PartitionKind::Other(val) => val,
        }
    }
}
