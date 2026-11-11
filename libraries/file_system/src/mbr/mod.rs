/*!
Master Boot Record (MBR) module.

This module provides functionality for working with MBR partition tables
backed by the `mbrman` crate.
*/

use alloc::vec::Vec;
use core::fmt;

use crate::{DirectBlockDevice, PartitionDevice, Size};

mod error;
mod partition;

pub use error::*;
pub use partition::*;

#[cfg(test)]
use crate::MemoryDevice;

/// Master Boot Record structure (512 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mbr {
    /// Underlying MBR header decoded/managed via `mbrman`.
    pub header: mbrman::MBRHeader,
    /// Partition table entries.
    pub partitions: [PartitionEntry; 4],
}

impl Mbr {
    /// MBR signature bytes (`[0x55, 0xAA]`).
    pub const SIGNATURE: [u8; 2] = [0x55, 0xAA];

    /// Size of MBR in bytes.
    pub const SIZE: usize = 512;

    /// Maximum number of primary partitions in MBR.
    pub const MAXIMUM_PARTITIONS_COUNT: usize = 4;

    /// Standard partition alignment block offset.
    pub const MINIMUM_START_BLOCK: Size = 2048;

    /// Create a new empty MBR with zero disk signature.
    pub fn new() -> Self {
        Self::new_with_signature(0)
    }

    /// Create a new empty MBR with a specified 32-bit disk signature.
    pub fn new_with_signature(disk_signature: u32) -> Self {
        let header = mbrman::MBRHeader::new(disk_signature.to_le_bytes());
        let partitions = [
            PartitionEntry::from_mbrman(&header.partition_1),
            PartitionEntry::from_mbrman(&header.partition_2),
            PartitionEntry::from_mbrman(&header.partition_3),
            PartitionEntry::from_mbrman(&header.partition_4),
        ];
        Self { header, partitions }
    }

    /// Parse an MBR from a 512-byte raw sector.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::BufferTooSmall);
        }

        let sector: &[u8; Self::SIZE] = data[..Self::SIZE]
            .try_into()
            .map_err(|_| Error::BufferTooSmall)?;

        let header = mbrman::MBRHeader::from_bytes(sector).map_err(Error::from)?;

        let partitions = [
            PartitionEntry::from_mbrman(&header.partition_1),
            PartitionEntry::from_mbrman(&header.partition_2),
            PartitionEntry::from_mbrman(&header.partition_3),
            PartitionEntry::from_mbrman(&header.partition_4),
        ];

        Ok(Self { header, partitions })
    }

    /// Encode the MBR into a 512-byte sector.
    pub fn to_bytes(&self) -> Result<[u8; Self::SIZE]> {
        let mut header = self.header.clone();
        header.partition_1 = self.partitions[0].to_mbrman();
        header.partition_2 = self.partitions[1].to_mbrman();
        header.partition_3 = self.partitions[2].to_mbrman();
        header.partition_4 = self.partitions[3].to_mbrman();
        Ok(header.to_bytes()?)
    }

    /// Read and parse the MBR from a block device.
    pub fn read_from_device(device: &impl DirectBlockDevice) -> Result<Self> {
        device.open()?;
        let mut buffer = [0u8; Self::SIZE];
        let seek_res = device.set_position(0, &crate::Position::Start(0));
        if let Err(e) = seek_res {
            let _ = device.close();
            return Err(e.into());
        }
        let read_res = device.read_sync(&mut buffer, 0);
        let _ = device.close();

        let bytes_read = read_res?;
        if bytes_read < Self::SIZE {
            return Err(Error::ReadFailed);
        }

        Self::from_bytes(&buffer)
    }

    /// Write the MBR to sector 0 of a block device.
    pub fn write_to_device(&self, device: &impl DirectBlockDevice) -> Result<()> {
        let buffer = self.to_bytes()?;

        device.open()?;
        let seek_res = device.set_position(0, &crate::Position::Start(0));
        if let Err(e) = seek_res {
            let _ = device.close();
            return Err(e.into());
        }
        let write_res = device.write_sync(&buffer, 0);
        let _ = device.close();

        let bytes_written = write_res?;
        if bytes_written < Self::SIZE {
            return Err(Error::WriteFailed);
        }

        Ok(())
    }

    /// Check if the MBR signature matches `[0x55, 0xAA]`.
    pub fn is_valid(&self) -> bool {
        self.header.boot_signature == Self::SIGNATURE
    }

    /// Get the 32-bit disk signature.
    pub fn get_disk_signature(&self) -> u32 {
        u32::from_le_bytes(self.header.disk_signature)
    }

    /// Set the 32-bit disk signature.
    pub fn set_disk_signature(&mut self, signature: u32) {
        self.header.disk_signature = signature.to_le_bytes();
    }

    /// Check for overlapping partitions.
    pub fn has_overlapping_partitions(&self) -> bool {
        for (i, p1) in self.partitions.iter().enumerate() {
            if !p1.is_valid() {
                continue;
            }
            for p2 in &self.partitions[i + 1..] {
                if p2.is_valid() && p1.overlaps_with(p2) {
                    return true;
                }
            }
        }
        false
    }

    /// Validate MBR signature, partition overlaps, and single-bootable rule.
    pub fn validate(&self) -> Result<()> {
        if !self.is_valid() {
            return Err(Error::InvalidSignature);
        }

        if self.has_overlapping_partitions() {
            return Err(Error::OverlappingPartitions);
        }

        let bootable_count = self.partitions.iter().filter(|p| p.bootable).count();
        if bootable_count > 1 {
            return Err(Error::MultipleBootablePartitions);
        }

        Ok(())
    }

    /// Add a partition to the first free slot.
    pub fn add_partition(
        &mut self,
        partition_type: PartitionKind,
        start_lba: u32,
        size_sectors: u32,
        bootable: bool,
    ) -> Result<usize> {
        let slot = self
            .partitions
            .iter()
            .position(|p| !p.is_valid())
            .ok_or(Error::Full)?;

        let new_partition =
            PartitionEntry::new_with_params(bootable, partition_type, start_lba, size_sectors);

        for existing in &self.partitions {
            if existing.is_valid() && new_partition.overlaps_with(existing) {
                return Err(Error::OverlappingPartitions);
            }
        }

        self.partitions[slot] = new_partition;
        Ok(slot)
    }

    /// Get all valid partitions.
    pub fn get_valid_partitions(&self) -> Vec<&PartitionEntry> {
        self.partitions.iter().filter(|p| p.is_valid()).collect()
    }

    /// Create an MBR with a single partition occupying available space after the 2048 sector alignment offset.
    pub fn create_basic(
        disk_signature: u32,
        partition_type: PartitionKind,
        block_count: u32,
    ) -> Result<Self> {
        if (block_count as u64) < Self::MINIMUM_START_BLOCK {
            return Err(Error::DeviceTooSmall);
        }

        let partition_block_count = block_count.saturating_sub(Self::MINIMUM_START_BLOCK as u32);
        if partition_block_count == 0 {
            return Err(Error::DeviceTooSmall);
        }

        let mut mbr = Self::new_with_signature(disk_signature);
        mbr.add_partition(
            partition_type,
            Self::MINIMUM_START_BLOCK as u32,
            partition_block_count,
            true,
        )?;

        Ok(mbr)
    }

    /// Format a disk with a specific disk signature and single partition, returning a `PartitionDevice`.
    pub fn format_disk_with_signature_and_partition<'a, D: DirectBlockDevice>(
        device: &'a D,
        disk_signature: u32,
        partition_type: PartitionKind,
    ) -> Result<PartitionDevice<'a, D>> {
        device.open()?;
        let block_count = device.get_block_count()?;
        device.close()?;

        if block_count < Self::MINIMUM_START_BLOCK {
            return Err(Error::DeviceTooSmall);
        }

        let mbr = Self::create_basic(disk_signature, partition_type, block_count as u32)?;
        mbr.write_to_device(device)?;

        create_partition_device(device, &mbr.partitions[0])
    }

    /// Find an existing partition with the specified disk signature, or format the disk.
    pub fn find_or_create_partition_with_signature<'a, D: DirectBlockDevice>(
        device: &'a D,
        target_signature: u32,
        partition_type: PartitionKind,
    ) -> Result<PartitionDevice<'a, D>> {
        if let Ok(existing_mbr) = Self::read_from_device(device)
            && existing_mbr.get_disk_signature() == target_signature
            && existing_mbr.is_valid()
            && let Some(valid_partition) = existing_mbr.partitions.iter().find(|p| p.is_valid())
        {
            return create_partition_device(device, valid_partition);
        }

        Self::format_disk_with_signature_and_partition(device, target_signature, partition_type)
    }
}

impl Default for Mbr {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Mbr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Master Boot Record:")?;
        writeln!(
            formatter,
            "  Disk Signature: 0x{:08X}",
            self.get_disk_signature()
        )?;
        writeln!(
            formatter,
            "  Boot Signature: 0x{:02X}{:02X}",
            self.header.boot_signature[1], self.header.boot_signature[0]
        )?;
        writeln!(formatter, "  Valid: {}", self.is_valid())?;
        writeln!(
            formatter,
            "  Partition Count: {}",
            self.partitions.iter().filter(|p| p.is_valid()).count()
        )?;
        writeln!(
            formatter,
            "  Has Overlaps: {}",
            self.has_overlapping_partitions()
        )?;
        writeln!(formatter, "  Partitions:")?;

        for (i, partition) in self.partitions.iter().enumerate() {
            if partition.is_valid() {
                writeln!(formatter, "    {}: {}", i + 1, partition)?;
            } else {
                writeln!(formatter, "    {}: Empty", i + 1)?;
            }
        }

        Ok(())
    }
}

/// Create a partition device from an MBR partition entry.
pub fn create_partition_device<'a, D: DirectBlockDevice>(
    device: &'a D,
    partition: &PartitionEntry,
) -> Result<PartitionDevice<'a, D>> {
    if !partition.is_valid() {
        return Err(Error::InvalidPartition);
    }

    device.open()?;
    let block_size = device.get_block_size()?;

    Ok(PartitionDevice::new(
        device,
        partition.start_block as _,
        partition.block_count as _,
        block_size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_legacy_sample_mbr_bytes() -> [u8; 512] {
        let mut data = [0u8; 512];
        data[510] = 0x55;
        data[511] = 0xAA;
        data[440..444].copy_from_slice(&0x12345678u32.to_le_bytes());

        // Partition 1: FAT32 LBA, bootable, starts at LBA 2048, size 204800 sectors
        let p1 = 446;
        data[p1] = 0x80;
        data[p1 + 4] = 0x0C;
        data[p1 + 8..p1 + 12].copy_from_slice(&2048u32.to_le_bytes());
        data[p1 + 12..p1 + 16].copy_from_slice(&204800u32.to_le_bytes());

        // Partition 2: Linux, non-bootable, starts at LBA 206848, size 102400 sectors
        let p2 = 446 + 16;
        data[p2 + 4] = 0x83;
        data[p2 + 8..p2 + 12].copy_from_slice(&206848u32.to_le_bytes());
        data[p2 + 12..p2 + 16].copy_from_slice(&102400u32.to_le_bytes());

        data
    }

    #[test]
    fn test_legacy_golden_bytes_round_trip() {
        let golden = create_legacy_sample_mbr_bytes();

        // 1. Legacy bytes parse with new implementation
        let mbr = Mbr::from_bytes(&golden).expect("new Mbr must parse legacy golden bytes");
        assert_eq!(mbr.get_disk_signature(), 0x12345678);
        assert!(mbr.is_valid());
        assert!(mbr.validate().is_ok());

        assert!(mbr.partitions[0].bootable);
        assert_eq!(mbr.partitions[0].kind, PartitionKind::Fat32Lba);
        assert_eq!(mbr.partitions[0].start_block, 2048);
        assert_eq!(mbr.partitions[0].block_count, 204800);

        assert!(!mbr.partitions[1].bootable);
        assert_eq!(mbr.partitions[1].kind, PartitionKind::Linux);
        assert_eq!(mbr.partitions[1].start_block, 206848);
        assert_eq!(mbr.partitions[1].block_count, 102400);

        assert!(!mbr.partitions[2].is_valid());
        assert!(!mbr.partitions[3].is_valid());

        // 2. New implementation serializes back to identical bytes
        let serialized = mbr.to_bytes().expect("serialize");
        assert_eq!(serialized, golden);

        // 3. Round-trip through mbrman directly
        let mbrman_header = mbrman::MBRHeader::from_bytes(&serialized)
            .expect("mbrman must parse serialized Mbr bytes");
        assert_eq!(mbrman_header.to_bytes().expect("mbrman to_bytes"), golden);
    }

    #[test]
    fn test_xila_formatted_mbr_round_trip() {
        let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
        let partition = Mbr::format_disk_with_signature_and_partition(
            &device,
            0x616C6958, // "Xila"
            PartitionKind::Xila,
        )
        .expect("format disk");

        assert_eq!(partition.get_start_lba(), 2048);
        assert_eq!(partition.get_block_count(), 8192 - 2048);

        let read_back = Mbr::read_from_device(&device).expect("read MBR");
        assert_eq!(read_back.get_disk_signature(), 0x616C6958);
        assert!(read_back.validate().is_ok());
        assert_eq!(read_back.partitions[0].kind, PartitionKind::Xila);
        assert!(read_back.partitions[0].bootable);
        assert_eq!(read_back.partitions[0].start_block, 2048);
        assert_eq!(read_back.partitions[0].block_count, 6144);
    }

    #[test]
    fn test_validation_rules() {
        let mut mbr = Mbr::new_with_signature(0x12345678);
        assert!(mbr.validate().is_ok());

        // Overlapping partitions
        mbr.partitions[0] =
            PartitionEntry::new_with_params(false, PartitionKind::Fat32Lba, 1000, 1000);
        mbr.partitions[1] =
            PartitionEntry::new_with_params(false, PartitionKind::Linux, 1500, 1000);
        assert_eq!(mbr.validate(), Err(Error::OverlappingPartitions));

        // Multiple bootable partitions
        mbr.partitions[1] = PartitionEntry::new_with_params(true, PartitionKind::Linux, 2500, 1000);
        mbr.partitions[0].bootable = true;
        assert_eq!(mbr.validate(), Err(Error::MultipleBootablePartitions));

        // Invalid signature
        mbr.header.boot_signature = [0, 0];
        assert_eq!(mbr.validate(), Err(Error::InvalidSignature));
    }

    #[test]
    fn test_find_or_create_partition_with_signature() {
        let device = MemoryDevice::<512>::new(4 * 1024 * 1024);

        // First call formats the device
        let p1 =
            Mbr::find_or_create_partition_with_signature(&device, 0x12345678, PartitionKind::Xila)
                .expect("find or create on empty disk");
        assert_eq!(p1.get_start_lba(), 2048);

        // Second call finds the existing partition
        let p2 =
            Mbr::find_or_create_partition_with_signature(&device, 0x12345678, PartitionKind::Xila)
                .expect("find existing partition");
        assert_eq!(p2.get_start_lba(), 2048);
    }
}
