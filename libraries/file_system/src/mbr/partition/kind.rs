//! Partition type definitions for MBR partition tables.
//!
//! This module provides the [`PartitionKind`] enumeration which defines
//! all standard partition types used in MBR partition tables. Each partition
//! type corresponds to a specific file system or partition purpose.

use core::fmt;

/// MBR partition type enumeration with comprehensive type definitions.
///
/// This enum represents the partition type field in MBR partition entries.
/// Each variant corresponds to a specific file system type or partition purpose
/// as defined by the standard PC partition type IDs.
///
/// # Standard Partition Types
///
/// ## FAT File Systems
/// - [`PartitionKind::Fat12`] - FAT12 file system (floppy disks, small partitions)
/// - [`PartitionKind::Fat16`] - FAT16 file system
/// - [`PartitionKind::Fat16Small`] - FAT16 for partitions < 32MB
/// - [`PartitionKind::Fat32`] - FAT32 file system
/// - [`PartitionKind::Fat32Lba`] - FAT32 with LBA addressing (recommended)
///
/// ## Extended Partitions
/// - [`PartitionKind::Extended`] - Extended partition (CHS addressing)
/// - [`PartitionKind::ExtendedLba`] - Extended partition (LBA addressing)
///
/// ## Linux File Systems
/// - [`PartitionKind::Linux`] - Linux native partition (typically ext2/3/4)
/// - [`PartitionKind::LinuxSwap`] - Linux swap partition
/// - [`PartitionKind::LinuxLvm`] - Linux LVM (Logical Volume Manager)
///
/// ## Other File Systems
/// - [`PartitionKind::NtfsExfat`] - NTFS or exFAT file system
/// - [`PartitionKind::EfiSystem`] - EFI System Partition
/// - [`PartitionKind::GptProtective`] - GPT protective partition
///
/// # Examples
///
/// ```rust
/// use file_system::mbr::PartitionKind;
///
/// let partition_type = PartitionKind::Fat32Lba;
/// assert!(partition_type.is_fat());
/// assert!(!partition_type.is_extended());
///
/// let linux_type = PartitionKind::from_u8(0x83);
/// assert_eq!(linux_type, PartitionKind::Linux);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PartitionKind {
    /// Empty/unused partition slot
    Empty = 0x00,
    /// FAT12 file system
    Fat12 = 0x01,
    /// FAT16 file system (small partitions < 32MB)
    Fat16Small = 0x04,
    /// Extended partition (CHS addressing)
    Extended = 0x05,
    /// FAT16 file system
    Fat16 = 0x06,
    /// NTFS or exFAT file system
    NtfsExfat = 0x07,
    /// FAT32 file system (CHS addressing)
    Fat32 = 0x0B,
    /// FAT32 file system (LBA addressing - recommended)
    Fat32Lba = 0x0C,
    /// FAT16 file system (LBA addressing)
    Fat16Lba = 0x0E,
    /// Extended partition (LBA addressing)
    ExtendedLba = 0x0F,
    /// Hidden FAT12 partition
    HiddenFat12 = 0x11,
    /// Hidden FAT16 partition (small)
    HiddenFat16Small = 0x14,
    /// Hidden FAT16 partition
    HiddenFat16 = 0x16,
    /// Hidden NTFS/exFAT partition
    HiddenNtfsExfat = 0x17,
    /// Hidden FAT32 partition
    HiddenFat32 = 0x1B,
    /// Hidden FAT32 partition (LBA addressing)
    HiddenFat32Lba = 0x1C,
    /// Hidden FAT16 partition (LBA addressing)
    HiddenFat16Lba = 0x1E,
    /// Linux swap partition
    LinuxSwap = 0x82,
    /// Linux native partition (typically ext2/3/4)
    Linux = 0x83,
    /// Linux LVM (Logical Volume Manager)
    LinuxLvm = 0x8E,
    /// GPT protective partition (indicates GPT, not MBR)
    GptProtective = 0xEE,
    /// EFI System Partition
    EfiSystem = 0xEF,
    /// Xila
    Xila = 0xDA,
    /// Unknown or custom partition type
    Unknown(u8),
}

impl PartitionKind {
    /// Create a partition type from a raw u8 value.
    ///
    /// This function maps standard partition type IDs to their corresponding
    /// enum variants. Unknown types are wrapped in [`PartitionKind::Unknown`].
    ///
    /// # Arguments
    ///
    /// * `Value` - The raw partition type ID from the MBR partition entry
    ///
    /// # Returns
    ///
    /// The corresponding partition type enum variant.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use file_system::mbr::PartitionKind;
    ///
    /// assert_eq!(PartitionKind::from_u8(0x0C), PartitionKind::Fat32Lba);
    /// assert_eq!(PartitionKind::from_u8(0x83), PartitionKind::Linux);
    ///
    /// // Unknown types are preserved
    /// if let PartitionKind::Unknown(id) = PartitionKind::from_u8(0xFF) {
    ///     assert_eq!(id, 0xFF);
    /// }
    /// ```
    pub fn from_u8(value: u8) -> Self {
        match value {
            0x00 => PartitionKind::Empty,
            0x01 => PartitionKind::Fat12,
            0x04 => PartitionKind::Fat16Small,
            0x05 => PartitionKind::Extended,
            0x06 => PartitionKind::Fat16,
            0x07 => PartitionKind::NtfsExfat,
            0x0B => PartitionKind::Fat32,
            0x0C => PartitionKind::Fat32Lba,
            0x0E => PartitionKind::Fat16Lba,
            0x0F => PartitionKind::ExtendedLba,
            0x11 => PartitionKind::HiddenFat12,
            0x14 => PartitionKind::HiddenFat16Small,
            0x16 => PartitionKind::HiddenFat16,
            0x17 => PartitionKind::HiddenNtfsExfat,
            0x1B => PartitionKind::HiddenFat32,
            0x1C => PartitionKind::HiddenFat32Lba,
            0x1E => PartitionKind::HiddenFat16Lba,
            0x82 => PartitionKind::LinuxSwap,
            0x83 => PartitionKind::Linux,
            0x8E => PartitionKind::LinuxLvm,
            0xEE => PartitionKind::GptProtective,
            0xEF => PartitionKind::EfiSystem,
            0xDA => PartitionKind::Xila,
            _ => PartitionKind::Unknown(value),
        }
    }

    /// Convert the partition type to its raw u8 value
    pub fn to_u8(&self) -> u8 {
        match self {
            PartitionKind::Empty => 0x00,
            PartitionKind::Fat12 => 0x01,
            PartitionKind::Fat16Small => 0x04,
            PartitionKind::Extended => 0x05,
            PartitionKind::Fat16 => 0x06,
            PartitionKind::NtfsExfat => 0x07,
            PartitionKind::Fat32 => 0x0B,
            PartitionKind::Fat32Lba => 0x0C,
            PartitionKind::Fat16Lba => 0x0E,
            PartitionKind::ExtendedLba => 0x0F,
            PartitionKind::HiddenFat12 => 0x11,
            PartitionKind::HiddenFat16Small => 0x14,
            PartitionKind::HiddenFat16 => 0x16,
            PartitionKind::HiddenNtfsExfat => 0x17,
            PartitionKind::HiddenFat32 => 0x1B,
            PartitionKind::HiddenFat32Lba => 0x1C,
            PartitionKind::HiddenFat16Lba => 0x1E,
            PartitionKind::LinuxSwap => 0x82,
            PartitionKind::Linux => 0x83,
            PartitionKind::LinuxLvm => 0x8E,
            PartitionKind::GptProtective => 0xEE,
            PartitionKind::EfiSystem => 0xEF,
            PartitionKind::Xila => 0xDA,
            PartitionKind::Unknown(value) => *value,
        }
    }

    /// Get the human-readable name of the partition type
    pub fn get_name(&self) -> &'static str {
        match self {
            PartitionKind::Empty => "Empty",
            PartitionKind::Fat12 => "FAT12",
            PartitionKind::Fat16Small => "FAT16 <32M",
            PartitionKind::Extended => "Extended",
            PartitionKind::Fat16 => "FAT16",
            PartitionKind::NtfsExfat => "NTFS/exFAT",
            PartitionKind::Fat32 => "FAT32",
            PartitionKind::Fat32Lba => "FAT32 LBA",
            PartitionKind::Fat16Lba => "FAT16 LBA",
            PartitionKind::ExtendedLba => "Extended LBA",
            PartitionKind::HiddenFat12 => "Hidden FAT12",
            PartitionKind::HiddenFat16Small => "Hidden FAT16 <32M",
            PartitionKind::HiddenFat16 => "Hidden FAT16",
            PartitionKind::HiddenNtfsExfat => "Hidden NTFS/exFAT",
            PartitionKind::HiddenFat32 => "Hidden FAT32",
            PartitionKind::HiddenFat32Lba => "Hidden FAT32 LBA",
            PartitionKind::HiddenFat16Lba => "Hidden FAT16 LBA",
            PartitionKind::LinuxSwap => "Linux swap",
            PartitionKind::Linux => "Linux",
            PartitionKind::LinuxLvm => "Linux LVM",
            PartitionKind::GptProtective => "GPT protective",
            PartitionKind::EfiSystem => "EFI System",
            PartitionKind::Xila => "Xila",
            PartitionKind::Unknown(_) => "Unknown",
        }
    }

    /// Check if this partition type is a FAT filesystem
    pub fn is_fat(&self) -> bool {
        matches!(
            self,
            PartitionKind::Fat12
                | PartitionKind::Fat16Small
                | PartitionKind::Fat16
                | PartitionKind::Fat32
                | PartitionKind::Fat32Lba
                | PartitionKind::Fat16Lba
                | PartitionKind::HiddenFat12
                | PartitionKind::HiddenFat16Small
                | PartitionKind::HiddenFat16
                | PartitionKind::HiddenFat32
                | PartitionKind::HiddenFat32Lba
                | PartitionKind::HiddenFat16Lba
        )
    }

    /// Check if this partition type is hidden
    pub fn is_hidden(&self) -> bool {
        matches!(
            self,
            PartitionKind::HiddenFat12
                | PartitionKind::HiddenFat16Small
                | PartitionKind::HiddenFat16
                | PartitionKind::HiddenNtfsExfat
                | PartitionKind::HiddenFat32
                | PartitionKind::HiddenFat32Lba
                | PartitionKind::HiddenFat16Lba
        )
    }

    /// Check if this partition type is an extended partition
    pub fn is_extended(&self) -> bool {
        matches!(self, PartitionKind::Extended | PartitionKind::ExtendedLba)
    }

    /// Check if this partition type is Linux-related
    pub fn is_linux(&self) -> bool {
        matches!(
            self,
            PartitionKind::Linux | PartitionKind::LinuxSwap | PartitionKind::LinuxLvm
        )
    }
}

impl fmt::Display for PartitionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PartitionKind::Unknown(value) => write!(formatter, "Unknown (0x{value:02X})"),
            _ => write!(formatter, "{} (0x{:02X})", self.get_name(), self.to_u8()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    #[test]
    fn test_partition_type_from_u8() {
        assert_eq!(PartitionKind::from_u8(0x00), PartitionKind::Empty);
        assert_eq!(PartitionKind::from_u8(0x0C), PartitionKind::Fat32Lba);
        assert_eq!(PartitionKind::from_u8(0x83), PartitionKind::Linux);
        assert_eq!(PartitionKind::from_u8(0xEE), PartitionKind::GptProtective);
        assert_eq!(PartitionKind::from_u8(0xFF), PartitionKind::Unknown(0xFF));
    }

    #[test]
    fn test_partition_type_to_u8() {
        assert_eq!(PartitionKind::Empty.to_u8(), 0x00);
        assert_eq!(PartitionKind::Fat32Lba.to_u8(), 0x0C);
        assert_eq!(PartitionKind::Linux.to_u8(), 0x83);
        assert_eq!(PartitionKind::GptProtective.to_u8(), 0xEE);
        assert_eq!(PartitionKind::Unknown(0xFF).to_u8(), 0xFF);
    }

    #[test]
    fn test_partition_type_round_trip() {
        let types = vec![
            0x00, 0x01, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x0C, 0x0E, 0x0F, 0x11, 0x14, 0x16, 0x17,
            0x1B, 0x1C, 0x1E, 0x82, 0x83, 0x8E, 0xEE, 0xEF, 0xFF, 0x42, 0x99,
        ];

        for type_value in types {
            let partition_type = PartitionKind::from_u8(type_value);
            assert_eq!(partition_type.to_u8(), type_value);
        }
    }

    #[test]
    fn test_partition_type_properties() {
        // Test FAT detection
        assert!(PartitionKind::Fat12.is_fat());
        assert!(PartitionKind::Fat16.is_fat());
        assert!(PartitionKind::Fat32.is_fat());
        assert!(PartitionKind::Fat32Lba.is_fat());
        assert!(PartitionKind::HiddenFat32.is_fat());
        assert!(!PartitionKind::Linux.is_fat());
        assert!(!PartitionKind::NtfsExfat.is_fat());

        // Test hidden detection
        assert!(PartitionKind::HiddenFat12.is_hidden());
        assert!(PartitionKind::HiddenFat32.is_hidden());
        assert!(PartitionKind::HiddenNtfsExfat.is_hidden());
        assert!(!PartitionKind::Fat32.is_hidden());
        assert!(!PartitionKind::Linux.is_hidden());

        // Test extended detection
        assert!(PartitionKind::Extended.is_extended());
        assert!(PartitionKind::ExtendedLba.is_extended());
        assert!(!PartitionKind::Fat32.is_extended());
        assert!(!PartitionKind::Linux.is_extended());

        // Test Linux detection
        assert!(PartitionKind::Linux.is_linux());
        assert!(PartitionKind::LinuxSwap.is_linux());
        assert!(PartitionKind::LinuxLvm.is_linux());
        assert!(!PartitionKind::Fat32.is_linux());
        assert!(!PartitionKind::NtfsExfat.is_linux());
    }

    #[test]
    fn test_partition_type_names() {
        assert_eq!(PartitionKind::Empty.get_name(), "Empty");
        assert_eq!(PartitionKind::Fat32Lba.get_name(), "FAT32 LBA");
        assert_eq!(PartitionKind::Linux.get_name(), "Linux");
        assert_eq!(PartitionKind::GptProtective.get_name(), "GPT protective");
        assert_eq!(PartitionKind::Unknown(0x42).get_name(), "Unknown");
    }

    #[test]
    fn test_partition_type_display() {
        let fat32_variant = PartitionKind::Fat32Lba;
        let display_string = format!("{fat32_variant}");
        assert!(display_string.contains("FAT32 LBA"));
        assert!(display_string.contains("0x0C"));

        let unknown = PartitionKind::Unknown(0x42);
        let unknown_string = format!("{unknown}");
        assert!(unknown_string.contains("Unknown"));
        assert!(unknown_string.contains("0x42"));
    }
}
