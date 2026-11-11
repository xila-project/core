/*!
Master Boot Record (MBR) module.

This module provides functionality for working with MBR partition tables
backed by the `mbrman` crate.
*/

use core::fmt;

use crate::{DirectBlockDevice, PartitionDevice, Position, Size};

mod error;
mod partition;

pub use error::*;
pub use partition::*;

#[cfg(test)]
use crate::MemoryDevice;

/// Master Boot Record structure (512 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mbr {
    /// Bootstrap code area (440 bytes).
    pub bootstrap_code: [u8; 440],
    /// 32-bit disk signature.
    pub disk_signature: [u8; 4],
    /// Copy protected bytes.
    pub copy_protected: [u8; 2],
    /// Partition table entries.
    pub partitions: [PartitionEntry; 4],
    /// Boot signature bytes (`[0x55, 0xAA]`).
    pub boot_signature: [u8; 2],
}

impl Mbr {
    /// Size of MBR in bytes.
    pub const SIZE: usize = 512;
    /// Standard partition alignment block offset.
    pub const MINIMUM_START_BLOCK: Size = 2048;

    /// Create a new empty MBR with a specified 32-bit disk signature.
    pub fn new_with_signature(disk_signature: u32) -> Self {
        Self {
            bootstrap_code: [0u8; 440],
            disk_signature: disk_signature.to_le_bytes(),
            copy_protected: [0u8; 2],
            partitions: [PartitionEntry::default(); 4],
            boot_signature: [0x55, 0xAA],
        }
    }

    /// Parse an MBR from a 512-byte raw sector.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let sector: &[u8; Self::SIZE] = data
            .get(..Self::SIZE)
            .and_then(|s| s.try_into().ok())
            .ok_or(Error::BufferTooSmall)?;

        let header = mbrman::MBRHeader::from_bytes(sector).map_err(Error::from)?;
        Ok(Self {
            bootstrap_code: header.bootstrap_code,
            disk_signature: header.disk_signature,
            copy_protected: header.copy_protected,
            partitions: [
                PartitionEntry::from_mbrman(&header.partition_1),
                PartitionEntry::from_mbrman(&header.partition_2),
                PartitionEntry::from_mbrman(&header.partition_3),
                PartitionEntry::from_mbrman(&header.partition_4),
            ],
            boot_signature: header.boot_signature,
        })
    }

    /// Encode the MBR into a 512-byte sector.
    pub fn to_bytes(&self) -> Result<[u8; Self::SIZE]> {
        let [p1, p2, p3, p4] = self.partitions;
        Ok(mbrman::MBRHeader {
            bootstrap_code: self.bootstrap_code,
            disk_signature: self.disk_signature,
            copy_protected: self.copy_protected,
            partition_1: p1.to_mbrman(),
            partition_2: p2.to_mbrman(),
            partition_3: p3.to_mbrman(),
            partition_4: p4.to_mbrman(),
            boot_signature: self.boot_signature,
        }
        .to_bytes()?)
    }

    /// Read and parse the MBR from a block device.
    pub fn read_from_device(device: &impl DirectBlockDevice) -> Result<Self> {
        device.open()?;
        let mut buffer = [0u8; Self::SIZE];
        let result = (|| {
            device.set_position(0, &Position::Start(0))?;
            if device.read_sync(&mut buffer, 0)? < Self::SIZE {
                return Err(Error::ReadFailed);
            }
            Self::from_bytes(&buffer)
        })();
        let _ = device.close();
        result
    }

    /// Write the MBR to sector 0 of a block device.
    pub fn write_to_device(&self, device: &impl DirectBlockDevice) -> Result<()> {
        let buffer = self.to_bytes()?;
        device.open()?;
        let result = (|| {
            device.set_position(0, &Position::Start(0))?;
            if device.write_sync(&buffer, 0)? < Self::SIZE {
                return Err(Error::WriteFailed);
            }
            Ok(())
        })();
        let _ = device.close();
        result
    }

    /// Check if the MBR signature matches `[0x55, 0xAA]`.
    pub fn is_valid(&self) -> bool {
        self.boot_signature == [0x55, 0xAA]
    }

    /// Get the 32-bit disk signature.
    pub fn get_disk_signature(&self) -> u32 {
        u32::from_le_bytes(self.disk_signature)
    }

    /// Validate MBR signature, partition overlaps, and single-bootable rule.
    pub fn validate(&self) -> Result<()> {
        if !self.is_valid() {
            return Err(Error::InvalidSignature);
        }
        for (i, p1) in self.partitions.iter().enumerate() {
            if p1.is_valid()
                && self.partitions[i + 1..]
                    .iter()
                    .any(|p2| p2.is_valid() && p1.overlaps_with(p2))
            {
                return Err(Error::OverlappingPartitions);
            }
        }
        if self.partitions.iter().filter(|p| p.bootable).count() > 1 {
            return Err(Error::MultipleBootablePartitions);
        }
        Ok(())
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

        let partition_block_count = (block_count - Self::MINIMUM_START_BLOCK) as u32;
        if partition_block_count == 0 {
            return Err(Error::DeviceTooSmall);
        }

        let mut mbr = Self::new_with_signature(disk_signature);
        mbr.partitions[0] = PartitionEntry {
            bootable: true,
            kind: partition_type,
            start_block: Self::MINIMUM_START_BLOCK as u32,
            block_count: partition_block_count,
            first_chs: mbrman::CHS::empty(),
            last_chs: mbrman::CHS::empty(),
        };
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

impl fmt::Display for Mbr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "MBR (signature: 0x{:08X}, partitions: {})",
            self.get_disk_signature(),
            self.partitions.iter().filter(|p| p.is_valid()).count()
        )
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
        data[440..444].copy_from_slice(&0x12345678u32.to_le_bytes());
        // Partition 1: FAT32 LBA (0x0C), bootable, starts at LBA 2048, size 204800 sectors
        data[446] = 0x80;
        data[450] = 0x0C;
        data[454..458].copy_from_slice(&2048u32.to_le_bytes());
        data[458..462].copy_from_slice(&204800u32.to_le_bytes());
        // Partition 2: Linux (0x83), non-bootable, starts at LBA 206848, size 102400 sectors
        data[466] = 0x83;
        data[470..474].copy_from_slice(&206848u32.to_le_bytes());
        data[474..478].copy_from_slice(&102400u32.to_le_bytes());
        data[510..512].copy_from_slice(&[0x55, 0xAA]);
        data
    }

    #[test]
    fn test_legacy_golden_bytes_round_trip() {
        let golden = create_legacy_sample_mbr_bytes();
        let mbr = Mbr::from_bytes(&golden).expect("parse legacy golden bytes");
        assert_eq!(mbr.get_disk_signature(), 0x12345678);
        assert!(mbr.is_valid() && mbr.validate().is_ok());

        assert!(mbr.partitions[0].bootable);
        assert_eq!(mbr.partitions[0].kind, PartitionKind::from(0x0C));
        assert_eq!(mbr.partitions[0].start_block, 2048);
        assert_eq!(mbr.partitions[0].block_count, 204800);

        assert!(!mbr.partitions[1].bootable);
        assert_eq!(mbr.partitions[1].kind, PartitionKind::from(0x83));
        assert_eq!(mbr.partitions[1].start_block, 206848);
        assert_eq!(mbr.partitions[1].block_count, 102400);

        assert!(!mbr.partitions[2].is_valid() && !mbr.partitions[3].is_valid());

        let serialized = mbr.to_bytes().expect("serialize");
        assert_eq!(serialized, golden);

        let mbrman_header = mbrman::MBRHeader::from_bytes(&serialized).expect("mbrman parse");
        assert_eq!(mbrman_header.to_bytes().expect("mbrman to_bytes"), golden);
    }

    #[test]
    fn test_xila_formatted_mbr_round_trip() {
        let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
        let partition =
            Mbr::format_disk_with_signature_and_partition(&device, 0x616C6958, PartitionKind::Xila)
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
        mbr.partitions[0] = PartitionEntry {
            bootable: false,
            kind: PartitionKind::Other(0x0C),
            start_block: 1000,
            block_count: 1000,
            ..Default::default()
        };
        mbr.partitions[1] = PartitionEntry {
            bootable: false,
            kind: PartitionKind::Other(0x83),
            start_block: 1500,
            block_count: 1000,
            ..Default::default()
        };
        assert_eq!(mbr.validate(), Err(Error::OverlappingPartitions));

        // Multiple bootable partitions
        mbr.partitions[1].start_block = 2500;
        mbr.partitions[1].bootable = true;
        mbr.partitions[0].bootable = true;
        assert_eq!(mbr.validate(), Err(Error::MultipleBootablePartitions));

        // Invalid signature
        mbr.boot_signature = [0, 0];
        assert_eq!(mbr.validate(), Err(Error::InvalidSignature));
    }

    #[test]
    fn test_find_or_create_partition_with_signature() {
        let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
        let p1 =
            Mbr::find_or_create_partition_with_signature(&device, 0x12345678, PartitionKind::Xila)
                .expect("format disk");
        assert_eq!(p1.get_start_lba(), 2048);

        let p2 =
            Mbr::find_or_create_partition_with_signature(&device, 0x12345678, PartitionKind::Xila)
                .expect("find partition");
        assert_eq!(p2.get_start_lba(), 2048);
    }
}
