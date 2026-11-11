//! Partition entry structures for MBR partition tables.

use crate::mbr::PartitionKind;

/// MBR partition table entry structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionEntry {
    /// Boot indicator.
    pub bootable: bool,
    /// Partition type.
    pub kind: PartitionKind,
    /// Starting LBA (Logical Block Address).
    pub start_block: u32,
    /// Size in sectors.
    pub block_count: u32,
    /// Legacy CHS address of the first sector.
    pub first_chs: mbrman::CHS,
    /// Legacy CHS address of the last sector.
    pub last_chs: mbrman::CHS,
}

impl Default for PartitionEntry {
    fn default() -> Self {
        Self {
            bootable: false,
            kind: PartitionKind::Empty,
            start_block: 0,
            block_count: 0,
            first_chs: mbrman::CHS::empty(),
            last_chs: mbrman::CHS::empty(),
        }
    }
}

impl PartitionEntry {
    /// Check if this partition entry is valid (non-empty and positive sector count).
    pub fn is_valid(&self) -> bool {
        self.kind != PartitionKind::Empty && self.block_count > 0
    }

    pub(crate) fn overlaps_with(&self, other: &Self) -> bool {
        if !self.is_valid() || !other.is_valid() {
            return false;
        }

        let (s1, e1) = (
            self.start_block as u64,
            self.start_block as u64 + self.block_count as u64 - 1,
        );
        let (s2, e2) = (
            other.start_block as u64,
            other.start_block as u64 + other.block_count as u64 - 1,
        );

        !(e1 < s2 || e2 < s1)
    }

    pub(crate) fn from_mbrman(entry: &mbrman::MBRPartitionEntry) -> Self {
        Self {
            bootable: entry.is_active(),
            kind: PartitionKind::from(entry.sys),
            start_block: entry.starting_lba,
            block_count: entry.sectors,
            first_chs: entry.first_chs,
            last_chs: entry.last_chs,
        }
    }

    pub(crate) fn to_mbrman(self) -> mbrman::MBRPartitionEntry {
        mbrman::MBRPartitionEntry {
            boot: if self.bootable {
                mbrman::BOOT_ACTIVE
            } else {
                mbrman::BOOT_INACTIVE
            },
            first_chs: self.first_chs,
            sys: self.kind.into(),
            last_chs: self.last_chs,
            starting_lba: self.start_block,
            sectors: self.block_count,
        }
    }
}
