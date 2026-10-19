//! Partition entry structures for MBR partition tables.
//!
//! This module provides the [`PartitionEntry`] structure which represents
//! individual partition entries in Master Boot Record (MBR) partition tables.
//! Each entry contains information about a partition's location, size, type, and bootability.

use core::fmt;

use shared::Unit;

use crate::mbr::PartitionKind;

/// MBR partition table entry structure (16 bytes).
///
/// This structure represents a single partition entry in an MBR partition table.
/// Each MBR contains exactly 4 partition entries, defining up to 4 primary partitions.
/// The structure follows the traditional PC BIOS partition table format.
///
/// # Memory Layout
///
/// The structure is packed and has a fixed 16-byte layout for MBR compatibility:
/// - Bytes 0: Boot indicator
/// - Bytes 1-3: CHS start address (legacy)
/// - Byte 4: Partition type ID
/// - Bytes 5-7: CHS end address (legacy)
/// - Bytes 8-11: LBA start address (little-endian)
/// - Bytes 12-15: Partition size in sectors (little-endian)
///
/// # Examples
///
/// ```rust
/// # extern crate alloc;
/// use file_system::mbr::{PartitionEntry, PartitionKind};
///
/// // Create a new bootable FAT32 partition
/// let partition = PartitionEntry::new_with_params(
///     true,
///     PartitionKind::Fat32Lba,
///     2048,
///     204800
/// );
///
/// assert!(partition.bootable);
/// assert_eq!(partition.start_block, 2048);
/// assert_eq!(partition.block_count, 204800);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PartitionEntry {
    /// Boot indicator
    pub bootable: bool,
    /// Starting head
    pub start_head: u8,
    /// Starting sector (bits 5-0) and cylinder high bits (bits 7-6)
    pub start_sector: u8,
    /// Starting cylinder (low 8 bits)
    pub start_cylinder: u8,
    /// Partition type ID
    pub kind: PartitionKind,
    /// Ending head
    pub end_head: u8,
    /// Ending sector (bits 5-0) and cylinder high bits (bits 7-6)
    pub end_sector: u8,
    /// Ending cylinder (low 8 bits)
    pub end_cylinder: u8,
    /// Starting LBA (Logical Block Address)
    pub start_block: u32,
    /// Size in sectors
    pub block_count: u32,
}

impl PartitionEntry {
    pub const SIZE: usize = 16;
    pub const BOOTABLE_FLAG: u8 = 0x80;

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            bootable: data[0] == Self::BOOTABLE_FLAG,
            start_head: data[1],
            start_sector: data[2],
            start_cylinder: data[3],
            kind: PartitionKind::from_u8(data[4]),
            end_head: data[5],
            end_sector: data[6],
            end_cylinder: data[7],
            start_block: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            block_count: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }

    /// Create a new empty (invalid) partition entry.
    ///
    /// All fields are initialized to zero, making this an invalid partition entry
    /// that will not be recognized by the MBR parser.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::mbr::PartitionEntry;
    ///
    /// let partition = PartitionEntry::new_empty();
    /// assert!(!partition.is_valid());
    /// assert!(!partition.bootable);
    /// ```
    pub fn new_empty() -> Self {
        Self {
            bootable: false,
            start_head: 0,
            start_sector: 0,
            start_cylinder: 0,
            kind: PartitionKind::Empty,
            end_head: 0,
            end_sector: 0,
            end_cylinder: 0,
            start_block: 0,
            block_count: 0,
        }
    }

    /// Create a new partition entry with specified parameters.
    ///
    /// This constructor creates a valid partition entry with the specified type,
    /// location, and size. The CHS (Cylinder-Head-Sector) fields are not set
    /// as modern systems use LBA addressing.
    ///
    /// # Arguments
    ///
    /// * `Bootable` - Whether this partition should be marked as bootable
    /// * `Partition_type` - The type of partition (FAT32, Linux, etc.)
    /// * `Start_lba` - Starting logical block address (sector number)
    /// * `Size_sectors` - Size of the partition in 512-byte sectors
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::mbr::{PartitionEntry, PartitionKind};
    ///
    /// // Create a 100MB FAT32 partition starting at sector 2048
    /// let partition = PartitionEntry::new_with_params(
    ///     true,
    ///     PartitionKind::Fat32Lba,
    ///     2048,
    ///     204800
    /// );
    ///
    /// assert!(partition.is_valid());
    /// assert!(partition.bootable);
    /// ```
    pub fn new_with_params(
        bootable: bool,
        kind: PartitionKind,
        start_lba: u32,
        size_sectors: u32,
    ) -> Self {
        let mut entry = Self::new_empty();
        entry.bootable = bootable;
        entry.kind = kind;
        entry.start_block = start_lba.to_le();
        entry.block_count = size_sectors.to_le();
        entry
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut data = [0u8; Self::SIZE];

        data[0] = if self.bootable {
            Self::BOOTABLE_FLAG
        } else {
            0x00
        };
        data[1] = self.start_head;
        data[2] = self.start_sector;
        data[3] = self.start_cylinder;
        data[4] = self.kind.to_u8();
        data[5] = self.end_head;
        data[6] = self.end_sector;
        data[7] = self.end_cylinder;
        data[8..12].copy_from_slice(&self.start_block.to_le_bytes());
        data[12..16].copy_from_slice(&self.block_count.to_le_bytes());

        data
    }

    /// Check if this partition entry is valid (non-zero)
    pub fn is_valid(&self) -> bool {
        self.kind != PartitionKind::Empty && self.block_count > 0
    }

    /// Check if this partition overlaps with another partition
    pub fn overlaps_with(&self, other: &Self) -> bool {
        if !self.is_valid() || !other.is_valid() {
            return false;
        }

        let self_start = self.start_block;
        let self_end = self.start_block + self.block_count - 1;
        let other_start = other.start_block;
        let other_end = other.start_block + other.block_count - 1;

        !(self_end < other_start || other_end < self_start)
    }

    /// Check if a given LBA is within this partition
    pub fn contains_lba(&self, lba: u32) -> bool {
        if !self.is_valid() {
            return false;
        }

        let start = self.start_block;
        let end = self.start_block + self.block_count - 1;
        lba >= start && lba <= end
    }

    /// Clear the partition entry (make it empty)
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
    use crate::mbr::Mbr;

    use super::{PartitionEntry, PartitionKind};
    use alloc::format;

    fn create_test_partition() -> PartitionEntry {
        PartitionEntry::new_with_params(
            true,                            // Bootable
            PartitionKind::Fat32Lba,         // Type
            Mbr::MINIMUM_START_BLOCK as u32, // Start LBA
            204800,                          // Size in sectors (100MB)
        )
    }

    #[test]
    fn test_partition_entry_new() {
        let entry = PartitionEntry::new_empty();
        assert!(!entry.is_valid());
        assert!(!entry.bootable);
        assert_eq!(entry.start_block, 0);
        assert_eq!(entry.block_count, 0);
        assert_eq!(entry.kind, PartitionKind::Empty);
    }

    #[test]
    fn test_partition_entry_new_with_params() {
        let entry = create_test_partition();
        assert!(entry.is_valid());
        assert!(entry.bootable);
        assert_eq!(entry.start_block, 2048);
        assert_eq!(entry.block_count, 204800);
        assert_eq!(entry.kind, PartitionKind::Fat32Lba);
    }

    #[test]
    fn test_partition_entry_overlaps() {
        let partition1 = PartitionEntry::new_with_params(false, PartitionKind::Fat32, 1000, 2000);
        let partition2 = PartitionEntry::new_with_params(false, PartitionKind::Linux, 2400, 1000);
        let partition3 =
            PartitionEntry::new_with_params(false, PartitionKind::LinuxSwap, 1500, 1000);

        // Partition1: 1000-2999, Partition2: 2400-3399, Partition3: 1500-2499
        assert!(partition1.overlaps_with(&partition3)); // 1000-2999 overlaps 1500-2499
        assert!(partition2.overlaps_with(&partition3)); // 2400-3399 overlaps 1500-2499 (overlap: 2400-2499)
        assert!(partition1.overlaps_with(&partition2)); // 1000-2999 overlaps 2400-3399 (overlap: 2400-2999)
    }

    #[test]
    fn test_partition_entry_no_overlap() {
        let partition1 = PartitionEntry::new_with_params(false, PartitionKind::Fat32, 1000, 1000);
        let partition2 = PartitionEntry::new_with_params(false, PartitionKind::Linux, 2000, 1000);

        // Partition1: 1000-1999, Partition2: 2000-2999
        assert!(!partition1.overlaps_with(&partition2));
        assert!(!partition2.overlaps_with(&partition1));
    }

    #[test]
    fn test_partition_entry_contains_lba() {
        let entry = create_test_partition();

        assert!(!entry.contains_lba(2047)); // Before start
        assert!(entry.contains_lba(2048)); // At start
        assert!(entry.contains_lba(100000)); // In middle
        assert!(entry.contains_lba(206847)); // At end (2048 + 204800 - 1)
        assert!(!entry.contains_lba(206848)); // After end
    }

    #[test]
    fn test_partition_entry_clear() {
        let mut entry = create_test_partition();
        assert!(entry.is_valid());

        entry.clear();
        assert!(!entry.is_valid());
        assert!(!entry.bootable);
        assert_eq!(entry.start_block, 0);
        assert_eq!(entry.block_count, 0);
    }

    #[test]
    fn test_partition_entry_default() {
        let entry = PartitionEntry::default();
        assert!(!entry.is_valid());
        assert_eq!(entry.kind, PartitionKind::Empty);
    }

    #[test]
    fn test_partition_entry_display() {
        let entry = create_test_partition();
        let display_string = format!("{entry}");

        assert!(display_string.contains("Type=0C"));
        assert!(display_string.contains("FAT32 LBA"));
        assert!(display_string.contains("Start_LBA=2048"));
        assert!(display_string.contains("Size=204800"));
        assert!(display_string.contains("Bootable=true"));

        let empty_entry = PartitionEntry::new_empty();
        let empty_string = format!("{empty_entry}");
        assert!(empty_string.contains("Empty partition"));
    }

    #[test]
    fn test_partition_entry_validity() {
        // Valid partition must have non-zero type and size
        let valid = PartitionEntry::new_with_params(false, PartitionKind::Linux, 100, 200);
        assert!(valid.is_valid());

        // Zero size makes it invalid
        let zero_size = PartitionEntry::new_with_params(false, PartitionKind::Linux, 100, 0);
        assert!(!zero_size.is_valid());

        // Empty type makes it invalid
        let empty_type = PartitionEntry::new_with_params(false, PartitionKind::Empty, 100, 200);
        assert!(!empty_type.is_valid());
    }
}
