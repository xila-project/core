//! Partition entry structures for MBR partition tables.

use core::fmt;

use shared::Unit;

use crate::mbr::PartitionKind;

/// MBR partition table entry structure.
///
/// Represents an entry in the MBR partition table.
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

impl PartitionEntry {
    /// Create a new empty partition entry.
    pub fn new_empty() -> Self {
        Self {
            bootable: false,
            kind: PartitionKind::Empty,
            start_block: 0,
            block_count: 0,
            first_chs: mbrman::CHS::empty(),
            last_chs: mbrman::CHS::empty(),
        }
    }

    /// Create a new partition entry with specified parameters.
    pub fn new_with_params(
        bootable: bool,
        kind: PartitionKind,
        start_lba: u32,
        size_sectors: u32,
    ) -> Self {
        Self {
            bootable,
            kind,
            start_block: start_lba,
            block_count: size_sectors,
            first_chs: mbrman::CHS::empty(),
            last_chs: mbrman::CHS::empty(),
        }
    }

    /// Convert from an `mbrman::MBRPartitionEntry`.
    pub fn from_mbrman(entry: &mbrman::MBRPartitionEntry) -> Self {
        Self {
            bootable: entry.is_active(),
            kind: PartitionKind::from_u8(entry.sys),
            start_block: entry.starting_lba,
            block_count: entry.sectors,
            first_chs: entry.first_chs,
            last_chs: entry.last_chs,
        }
    }

    /// Convert to an `mbrman::MBRPartitionEntry`.
    pub fn to_mbrman(&self) -> mbrman::MBRPartitionEntry {
        mbrman::MBRPartitionEntry {
            boot: if self.bootable {
                mbrman::BOOT_ACTIVE
            } else {
                mbrman::BOOT_INACTIVE
            },
            first_chs: self.first_chs,
            sys: self.kind.to_u8(),
            last_chs: self.last_chs,
            starting_lba: self.start_block,
            sectors: self.block_count,
        }
    }

    /// Check if this partition entry is valid (non-empty and positive sector count).
    pub fn is_valid(&self) -> bool {
        self.kind != PartitionKind::Empty && self.block_count > 0
    }

    /// Check if this partition overlaps with another partition.
    pub fn overlaps_with(&self, other: &Self) -> bool {
        if !self.is_valid() || !other.is_valid() {
            return false;
        }

        let self_start = self.start_block as u64;
        let self_end = self_start + self.block_count as u64 - 1;
        let other_start = other.start_block as u64;
        let other_end = other_start + other.block_count as u64 - 1;

        !(self_end < other_start || other_end < self_start)
    }

    /// Check if a given LBA is within this partition.
    pub fn contains_lba(&self, lba: u32) -> bool {
        if !self.is_valid() {
            return false;
        }

        let start = self.start_block;
        let end = self
            .start_block
            .saturating_add(self.block_count)
            .saturating_sub(1);
        lba >= start && lba <= end
    }

    /// Clear the partition entry (make it empty).
    pub fn clear(&mut self) {
        *self = Self::new_empty();
    }
}

impl Default for PartitionEntry {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl fmt::Display for PartitionEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_valid() {
            write!(formatter, "Empty partition")
        } else {
            write!(
                formatter,
                "Partition: Type={:02X} ({}), Start_LBA={}, Size={} sectors ({}), Bootable={}",
                self.kind.to_u8(),
                self.kind,
                self.start_block,
                self.block_count,
                Unit::new(self.block_count * 512, "B"),
                self.bootable
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_entry_validity() {
        let empty = PartitionEntry::new_empty();
        assert!(!empty.is_valid());

        let valid = PartitionEntry::new_with_params(true, PartitionKind::Xila, 2048, 4096);
        assert!(valid.is_valid());
        assert!(valid.bootable);
        assert_eq!(valid.start_block, 2048);
        assert_eq!(valid.block_count, 4096);
    }

    #[test]
    fn test_partition_entry_overlaps() {
        let p1 = PartitionEntry::new_with_params(false, PartitionKind::Fat32Lba, 1000, 500);
        let p2 = PartitionEntry::new_with_params(false, PartitionKind::Linux, 1200, 500);
        let p3 = PartitionEntry::new_with_params(false, PartitionKind::Xila, 2000, 500);

        assert!(p1.overlaps_with(&p2));
        assert!(p2.overlaps_with(&p1));
        assert!(!p1.overlaps_with(&p3));
        assert!(!p2.overlaps_with(&p3));
    }

    #[test]
    fn test_mbrman_conversion() {
        let entry = PartitionEntry::new_with_params(true, PartitionKind::Xila, 2048, 6144);
        let mbr_entry = entry.to_mbrman();
        assert!(mbr_entry.is_active());
        assert_eq!(mbr_entry.sys, 0xDA);
        assert_eq!(mbr_entry.starting_lba, 2048);
        assert_eq!(mbr_entry.sectors, 6144);

        let round_trip = PartitionEntry::from_mbrman(&mbr_entry);
        assert_eq!(entry, round_trip);
    }
}
