/*!
Master Boot Record (MBR) module

This module provides functionality for working with MBR (Master Boot Record)
partition tables, which are used in traditional BIOS-based systems.

# Features

- Parse and validate MBR structures from raw bytes or devices
- Create and modify MBR partition tables
- Work with individual partition entries
- Create partition devices for accessing individual partitions
- Utility functions for common MBR operations
- Type-safe partition type enumeration

# Examples

```rust
extern crate alloc;

use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind, create_partition_device}};

let device = MemoryDevice::<512>::new(4 * 1024 * 1024);

// Create a new MBR
let mut mbr = Mbr::new_with_signature(0x12345678);
// Add a FAT32 partition
mbr.add_partition(PartitionKind::Fat32Lba, 2048, 204800, true).unwrap();

// Write MBR to the device
mbr.write_to_device(&device).unwrap();

// Read MBR from a device
let mbr = Mbr::read_from_device(&device).unwrap();

// Display MBR information
println!("{}", mbr);

// Get all valid partitions
let partitions = mbr.get_valid_partitions();

// Create a partition device
if let Some(partition) = partitions.first() {
    let partition_device = create_partition_device(&device, partition).unwrap();
}
```
*/

use alloc::vec::Vec;
use core::fmt;

use crate::{DirectBlockDevice, PartitionDevice, Size};

mod error;
mod partition;
mod utilities;
pub use error::*;
pub use partition::*;
pub use utilities::*;

#[cfg(test)]
use crate::MemoryDevice;

/// Master Boot Record structure (512 bytes)
#[derive(Debug, Clone)]
pub struct Mbr {
    /// Bootstrap code (440 bytes)
    pub bootstrap_code: [u8; 440],
    /// Optional disk signature (4 bytes)
    pub disk_signature: [u8; 4],
    /// Reserved (usually 0x0000)
    pub reserved: [u8; 2],
    /// Partition table (4 entries × 16 bytes = 64 bytes)
    pub partitions: [PartitionEntry; 4],
    /// Boot signature (0x55AA)
    pub boot_signature: [u8; 2],
}

impl Mbr {
    /// MBR signature bytes
    pub const SIGNATURE: [u8; 2] = [0x55, 0xAA];

    /// Size of MBR in bytes
    pub const SIZE: usize = 512;

    /// Maximum number of primary partitions in MBR
    pub const MAXIMUM_PARTITIONS_COUNT: usize = 4;

    pub const MINIMUM_START_BLOCK: Size = 2048;

    /// Create a new empty MBR
    pub fn new() -> Self {
        Self {
            bootstrap_code: [0; 440],
            disk_signature: [0; 4],
            reserved: [0; 2],
            partitions: [PartitionEntry::new_empty(); 4],
            boot_signature: Self::SIGNATURE,
        }
    }

    /// Create a new MBR with a specific disk signature
    pub fn new_with_signature(disk_signature: u32) -> Self {
        let mut mbr = Self::new();
        mbr.set_disk_signature(disk_signature);
        mbr
    }

    /// Parse MBR from raw bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::BufferTooSmall);
        }

        // Check MBR signature
        if data[510] != Self::SIGNATURE[0] || data[511] != Self::SIGNATURE[1] {
            return Err(Error::InvalidSignature);
        }

        let mut mbr = Mbr {
            bootstrap_code: [0; 440],
            disk_signature: [0; 4],
            reserved: [0; 2],
            partitions: [PartitionEntry::new_empty(); 4],
            boot_signature: [0; 2],
        };

        // Copy bootstrap code
        mbr.bootstrap_code.copy_from_slice(&data[0..440]);

        // Copy disk signature
        mbr.disk_signature.copy_from_slice(&data[440..444]);

        // Copy reserved bytes
        mbr.reserved.copy_from_slice(&data[444..446]);

        // Parse partition entries
        for (i, partition) in mbr.partitions.iter_mut().enumerate() {
            let offset = 446 + (i * 16);
            let partition_data = &data[offset..offset + 16];

            *partition = PartitionEntry::parse(partition_data).unwrap();
        }

        // Copy boot signature
        mbr.boot_signature.copy_from_slice(&data[510..512]);

        Ok(mbr)
    }

    /// Read and parse MBR from a device
    pub fn read_from_device(device: &impl DirectBlockDevice) -> Result<Self> {
        // Read the first 512 bytes (MBR sector)

        device.open()?;

        let mut buffer = [0u8; Self::SIZE];
        device.set_position(0, &crate::Position::Start(0))?;
        let bytes_read = device.read_sync(&mut buffer, 0)?;

        if bytes_read < Self::SIZE {
            return Err(Error::ReadFailed);
        }

        device.close()?;

        Self::from_bytes(&buffer)
    }

    /// Write MBR to a device
    pub fn write_to_device(&self, device: &impl DirectBlockDevice) -> Result<()> {
        // Set position to the beginning of the device

        device.open()?;

        device.set_position(0, &crate::Position::Start(0))?;

        // Convert to bytes and write
        let buffer = self.to_bytes();
        let bytes_written = device.write_sync(&buffer, 0)?;

        if bytes_written < Self::SIZE {
            return Err(Error::WriteFailed);
        }

        device.close()?;

        Ok(())
    }

    /// Check if MBR has a valid signature
    pub fn is_valid(&self) -> bool {
        self.boot_signature == Self::SIGNATURE
    }

    /// Get all valid partitions
    pub fn get_valid_partitions(&self) -> Vec<&PartitionEntry> {
        self.partitions
            .iter()
            .filter(|partition| partition.is_valid())
            .collect()
    }

    /// Get all valid partitions (mutable)
    pub fn get_valid_partitions_mut(&mut self) -> Vec<&mut PartitionEntry> {
        self.partitions
            .iter_mut()
            .filter(|partition| partition.is_valid())
            .collect()
    }

    /// Get bootable partition (if any)
    pub fn get_bootable_partition(&self) -> Option<&PartitionEntry> {
        self.partitions.iter().find(|partition| partition.bootable)
    }

    /// Get bootable partition (mutable, if any)
    pub fn get_bootable_partition_mut(&mut self) -> Option<&mut PartitionEntry> {
        self.partitions
            .iter_mut()
            .find(|partition| partition.bootable)
    }

    /// Set a partition as bootable (clears bootable flag from other partitions)
    pub fn set_bootable_partition(&mut self, index: usize) -> Result<()> {
        if index >= Self::MAXIMUM_PARTITIONS_COUNT {
            return Err(Error::InvalidIndex);
        }

        // Clear bootable flag from all partitions
        for partition in &mut self.partitions {
            partition.bootable = false;
        }

        // Set the specified partition as bootable
        self.partitions[index].bootable = true;
        Ok(())
    }

    /// Check if this MBR contains a GPT protective partition
    pub fn has_gpt_protective_partition(&self) -> bool {
        self.partitions
            .iter()
            .any(|partition| partition.kind == PartitionKind::GptProtective)
    }

    /// Get disk signature as u32
    pub fn get_disk_signature(&self) -> u32 {
        u32::from_le_bytes(self.disk_signature)
    }

    /// Set disk signature
    pub fn set_disk_signature(&mut self, signature: u32) {
        self.disk_signature = signature.to_le_bytes();
    }

    /// Get the first free partition slot
    pub fn get_free_partition_slot(&self) -> Option<usize> {
        self.partitions
            .iter()
            .position(|partition| !partition.is_valid())
    }

    /// Add a new partition
    pub fn add_partition(
        &mut self,
        partition_type: PartitionKind,
        start_lba: u32,
        size_sectors: u32,
        bootable: bool,
    ) -> Result<usize> {
        let slot = self.get_free_partition_slot().ok_or(Error::Full)?;

        let new_partition =
            PartitionEntry::new_with_params(bootable, partition_type, start_lba, size_sectors);

        // Check for overlaps with existing partitions
        for existing in &self.partitions {
            if existing.is_valid() && new_partition.overlaps_with(existing) {
                return Err(Error::OverlappingPartitions);
            }
        }

        self.partitions[slot] = new_partition;

        // If this is the only bootable partition or no other bootable partition exists
        if bootable {
            self.set_bootable_partition(slot)?;
        }

        Ok(slot)
    }

    /// Remove a partition by index
    pub fn remove_partition(&mut self, index: usize) -> Result<()> {
        if index >= Self::MAXIMUM_PARTITIONS_COUNT {
            return Err(Error::InvalidIndex);
        }

        self.partitions[index].clear();
        Ok(())
    }

    /// Check for partition overlaps
    pub fn has_overlapping_partitions(&self) -> bool {
        let valid_partitions = self.get_valid_partitions();

        for (i, partition1) in valid_partitions.iter().enumerate() {
            for partition2 in valid_partitions.iter().skip(i + 1) {
                if partition1.overlaps_with(partition2) {
                    return true;
                }
            }
        }

        false
    }

    /// Get partition count
    pub fn get_partition_count(&self) -> usize {
        self.partitions.iter().filter(|p| p.is_valid()).count()
    }

    /// Create partition devices for all valid partitions in this MBR.
    ///
    /// This method iterates through all partition entries in this MBR and creates
    /// [`PartitionDevice`] instances for each valid partition. This is useful
    /// when you need to access all partitions on a disk programmatically.
    ///
    /// # Arguments
    ///
    /// * `base_device` - The underlying storage device containing all partitions
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<Partition_device_type>)` - Vector of partition devices for all valid partitions
    /// * `Err(Error)` - Error if any partition device creation fails
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    /// // Create an MBR with multiple partitions
    /// let mut mbr = Mbr::new_with_signature(0x12345678);
    /// mbr.add_partition(PartitionKind::Fat32Lba, 2048, 1024, true).unwrap();
    /// mbr.add_partition(PartitionKind::Linux, 4096, 2048, false).unwrap();
    /// mbr.write_to_device(&device).unwrap();
    ///
    /// // Read it back and create all partition devices
    /// let mbr = Mbr::read_from_device(&device).unwrap();
    /// let partition_devices = mbr.create_all_partition_devices(&device).unwrap();
    /// println!("Created {} partition devices", partition_devices.len());
    ///
    /// for (i, partition) in partition_devices.iter().enumerate() {
    ///     println!("Partition {}: {} blocks", i, partition.get_block_count());
    /// }
    /// ```
    pub fn create_all_partition_devices<'a, D: DirectBlockDevice>(
        &self,
        base_device: &'a D,
    ) -> Result<Vec<PartitionDevice<'a, D>>> {
        let mut devices = Vec::new();

        for partition in &self.partitions {
            if partition.is_valid() {
                let device = create_partition_device(base_device, partition)?;
                devices.push(device);
            }
        }

        Ok(devices)
    }

    /// Find partitions of a specific type within this MBR.
    ///
    /// This method searches through all partitions in this MBR and returns references
    /// to those that match the specified partition type. This is useful for locating
    /// specific types of partitions (e.g., FAT32, Linux, etc.) without creating
    /// partition devices.
    ///
    /// # Arguments
    ///
    /// * `partition_type` - The specific partition type to find
    ///
    /// # Returns
    ///
    /// A vector of tuples containing the partition index and reference to the partition entry
    /// for each matching partition.
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{PartitionKind, Mbr, PartitionEntry}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    /// // Create an MBR with FAT32 partition
    /// let mut mbr = Mbr::new_with_signature(0x12345678);
    /// mbr.add_partition(PartitionKind::Fat32Lba, 2048, 1024, true).unwrap();
    /// mbr.write_to_device(&device).unwrap();
    ///
    /// // Read it back and find FAT32 partitions
    /// let mbr = Mbr::read_from_device(&device).unwrap();
    /// let fat32_partitions = mbr.find_partitions_by_type(PartitionKind::Fat32Lba);
    /// println!("Found {} FAT32 partitions", fat32_partitions.len());
    /// ```
    pub fn find_partitions_by_type(&self, kind: PartitionKind) -> Vec<(usize, &PartitionEntry)> {
        self.partitions
            .iter()
            .enumerate()
            .filter(|(_, partition)| partition.is_valid() && partition.kind == kind)
            .collect()
    }

    /// Validate this MBR structure for consistency and correctness.
    ///
    /// This method performs comprehensive validation of this MBR structure, checking:
    /// - MBR signature validity (0x55AA boot signature)
    /// - Partition overlap detection
    /// - Bootable partition count (at most one partition should be bootable)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - MBR is valid and consistent
    /// * `Err(Error::Corrupted)` - MBR is invalid or corrupted
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    /// // First create and write a valid MBR
    /// let mut mbr = Mbr::new_with_signature(0x12345678);
    /// mbr.add_partition(PartitionKind::Fat32Lba, 2048, 1024, true).unwrap();
    /// mbr.write_to_device(&device).unwrap();
    ///
    /// // Read it back and validate
    /// let mbr = Mbr::read_from_device(&device).unwrap();
    /// match mbr.validate() {
    ///     Ok(()) => println!("MBR is valid"),
    ///     Err(e) => println!("Validation error: {}", e),
    /// }
    /// ```
    pub fn validate(&self) -> Result<()> {
        // Check MBR signature
        if !self.is_valid() {
            return Err(Error::InvalidSignature);
        }

        // Check for overlapping partitions
        if self.has_overlapping_partitions() {
            return Err(Error::OverlappingPartitions);
        }

        // Check that only one partition is bootable
        let bootable_count = self.partitions.iter().filter(|p| p.bootable).count();

        if bootable_count > 1 {
            return Err(Error::MultipleBootablePartitions);
        }

        Ok(())
    }

    /// Generate comprehensive statistics from this MBR.
    ///
    /// This method analyzes all partitions in this MBR and generates
    /// detailed statistics about partition types, sizes, and other characteristics.
    ///
    /// # Returns
    ///
    /// A new `PartitionStatistics` containing the computed statistics.
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    /// // Create an MBR with some partitions
    /// let mut mbr = Mbr::new_with_signature(0x12345678);
    /// mbr.add_partition(PartitionKind::Fat32Lba, 2048, 1024, true).unwrap();
    /// mbr.add_partition(PartitionKind::Linux, 4096, 2048, false).unwrap();
    /// mbr.write_to_device(&device).unwrap();
    ///
    /// // Read it back and analyze
    /// let mbr = Mbr::read_from_device(&device).unwrap();
    /// let stats = mbr.get_statistics();
    /// if stats.total_partitions > 0 {
    ///     println!("Average partition size: {} sectors",
    ///              stats.total_used_sectors / stats.total_partitions as u64);
    /// }
    /// ```
    pub fn get_statistics(&self) -> PartitionStatistics {
        let valid_partitions: Vec<_> = self.get_valid_partitions();

        let total_partitions = valid_partitions.len();
        let bootable_partitions = valid_partitions.iter().filter(|p| p.bootable).count();

        let fat_partitions = valid_partitions.iter().filter(|p| p.kind.is_fat()).count();

        let linux_partitions = valid_partitions
            .iter()
            .filter(|p| p.kind.is_linux())
            .count();

        let hidden_partitions = valid_partitions
            .iter()
            .filter(|p| p.kind.is_hidden())
            .count();

        let extended_partitions = valid_partitions
            .iter()
            .filter(|p| p.kind.is_extended())
            .count();

        let unknown_partitions = valid_partitions
            .iter()
            .filter(|p| matches!(p.kind, PartitionKind::Unknown(_)))
            .count();

        let total_used_sectors = valid_partitions.iter().map(|p| p.block_count as Size).sum();

        let largest_partition_sectors = valid_partitions
            .iter()
            .map(|p| p.block_count)
            .max()
            .unwrap_or(0);

        let smallest_partition_sectors = valid_partitions
            .iter()
            .map(|p| p.block_count)
            .min()
            .unwrap_or(0);

        PartitionStatistics {
            total_partitions,
            bootable_partitions,
            fat_partitions,
            linux_partitions,
            hidden_partitions,
            extended_partitions,
            unknown_partitions,
            total_used_sectors,
            largest_partition_sectors,
            smallest_partition_sectors,
        }
    }

    /// Create a basic MBR with a single partition covering most of the disk.
    ///
    /// This function creates a simple MBR structure with one partition that uses
    /// most of the available disk space. It leaves 2048 sectors at the beginning
    /// for proper alignment, which is standard practice for modern storage devices.
    ///
    /// # Arguments
    ///
    /// * `disk_signature` - Unique 32-bit signature for the disk
    /// * `partition_type` - Type of partition to create (e.g., FAT32, Linux, etc.)
    /// * `total_sectors` - Total number of sectors available on the disk
    ///
    /// # Returns
    ///
    /// * `Ok(MBR_type)` - Successfully created MBR with single partition
    /// * `Err(Error::InvalidParameter)` - Disk is too small for a partition
    ///
    /// # Examples
    ///
    /// ```rust
    /// use file_system::mbr::{Mbr, PartitionKind};
    ///
    /// // Create MBR for a 4MB device (8192 blocks)
    /// let mbr = Mbr::create_basic(0x12345678, PartitionKind::Fat32Lba, 8192).unwrap();
    ///
    /// // The MBR will have one FAT32 partition starting at block 2048
    /// let partitions = mbr.get_valid_partitions();
    /// assert_eq!(partitions.len(), 1);
    /// assert_eq!(partitions[0].start_block, 2048);
    /// ```
    pub fn create_basic(
        disk_signature: u32,
        partition_type: PartitionKind,
        block_count: u32,
    ) -> Result<Self> {
        let mut mbr = Self::new_with_signature(disk_signature);

        // Leave some space at the beginning (typically 2048 sectors for alignment)
        let partition_block_count = block_count.saturating_sub(Self::MINIMUM_START_BLOCK as u32);

        if partition_block_count == 0 {
            return Err(Error::DeviceTooSmall);
        }

        mbr.add_partition(
            partition_type,
            Self::MINIMUM_START_BLOCK as u32,
            partition_block_count,
            true,
        )?;

        Ok(mbr)
    }

    /// Convert MBR back to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buffer = [0u8; Self::SIZE];

        // Copy bootstrap code
        buffer[0..440].copy_from_slice(&self.bootstrap_code);

        // Copy disk signature
        buffer[440..444].copy_from_slice(&self.disk_signature);

        // Copy reserved bytes
        buffer[444..446].copy_from_slice(&self.reserved);

        // Copy partition entries
        for (i, partition) in self.partitions.iter().enumerate() {
            let offset = 446 + (i * 16);
            buffer[offset..offset + 16].copy_from_slice(&partition.to_bytes());
        }

        // Copy boot signature
        buffer[510..512].copy_from_slice(&self.boot_signature);

        buffer
    }

    /// Find or create a partition with a specific disk signature.
    ///
    /// This function searches for an MBR with the specified disk signature on the device.
    /// If found, it returns a partition device for the first valid partition. If not found,
    /// or if no valid partitions exist, it formats the disk with a new MBR containing a
    /// single partition that occupies the full disk space.
    ///
    /// # Arguments
    ///
    /// * `device` - The storage device to search or format
    /// * `target_signature` - The disk signature to look for
    /// * `partition_type` - The type of partition to create if formatting is needed
    ///
    /// # Returns
    ///
    /// * `Ok(Partition_device_type)` - Partition device for the found or created partition
    /// * `Err(Error)` - Error if operation failed
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    ///
    /// // Look for partition with signature 0x12345678, create FAT32 partition if not found
    /// let partition = Mbr::find_or_create_partition_with_signature(
    ///     &device,
    ///     0x12345678,
    ///     PartitionKind::Fat32Lba
    /// ).unwrap();
    ///
    /// // The partition device is ready to use
    /// assert!(partition.is_valid());
    /// ```
    pub fn find_or_create_partition_with_signature<'a, D: DirectBlockDevice>(
        device: &'a D,
        target_signature: u32,
        partition_type: PartitionKind,
    ) -> Result<PartitionDevice<'a, D>> {
        // Try to read existing MBR from device
        if let Ok(existing_mbr) = Self::read_from_device(device) {
            // Check if the MBR has the target signature
            if existing_mbr.get_disk_signature() == target_signature && existing_mbr.is_valid() {
                // Get valid partitions
                let valid_partitions = existing_mbr.get_valid_partitions();

                // If we have at least one valid partition, return it
                if !valid_partitions.is_empty() {
                    return create_partition_device(device, valid_partitions[0]);
                }
            }
        }

        // Either no MBR found, wrong signature, or no valid partitions
        // Format the disk with new MBR and create partition
        Self::format_disk_with_signature_and_partition(device, target_signature, partition_type)
    }

    /// Format a disk with a specific signature and create a single partition.
    ///
    /// This function creates a new MBR with the specified disk signature and a single
    /// partition that occupies most of the available disk space, leaving standard
    /// alignment space at the beginning.
    ///
    /// # Arguments
    ///
    /// * `device` - The storage device to format
    /// * `disk_signature` - The disk signature to set in the new MBR
    /// * `partition_type` - The type of partition to create
    ///
    /// # Returns
    ///
    /// * `Ok(Partition_device_type)` - Partition device for the created partition
    /// * `Err(Error)` - Error if operation failed
    ///
    /// # Examples
    ///
    /// ```rust
    /// extern crate alloc;
    /// use file_system::{MemoryDevice, mbr::{Mbr, PartitionKind}};
    ///
    /// let device = MemoryDevice::<512>::new(4 * 1024 * 1024);
    ///
    /// // Format disk and create a Linux partition with specific signature
    /// let partition = Mbr::format_disk_with_signature_and_partition(
    ///     &device,
    ///     0xABCDEF00,
    ///     PartitionKind::Linux
    /// ).unwrap();
    ///
    /// // Verify the partition was created correctly
    /// assert!(partition.is_valid());
    /// assert_eq!(partition.get_start_lba(), 2048); // Standard alignment
    /// ```
    pub fn format_disk_with_signature_and_partition<'a, D: DirectBlockDevice>(
        device: &'a D,
        disk_signature: u32,
        partition_type: PartitionKind,
    ) -> Result<PartitionDevice<'a, D>> {
        // Get device size in sectors
        device.open()?;
        let block_count = device.get_block_count()?;
        device.close()?;

        // Ensure device is large enough for a meaningful partition
        if block_count < 2048 {
            return Err(Error::DeviceTooSmall);
        }

        // Create new MBR with the specified signature
        let new_mbr = Self::create_basic(disk_signature, partition_type, block_count as _)?;

        // Write the new MBR to device
        new_mbr.write_to_device(device)?;

        // Get the first (and only) partition
        let valid_partitions = new_mbr.get_valid_partitions();
        if valid_partitions.is_empty() {
            return Err(Error::NoValidPartitions);
        }

        // Create and return partition device
        create_partition_device(device, valid_partitions[0])
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
            self.boot_signature[1], self.boot_signature[0]
        )?;
        writeln!(formatter, "  Valid: {}", self.is_valid())?;
        writeln!(
            formatter,
            "  GPT Protective: {}",
            self.has_gpt_protective_partition()
        )?;
        writeln!(
            formatter,
            "  Partition Count: {}",
            self.get_partition_count()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mbr::PartitionEntry;
    use alloc::{format, vec};

    fn create_sample_mbr_bytes() -> [u8; 512] {
        let mut data = [0u8; 512];

        // Set MBR signature
        data[510] = 0x55;
        data[511] = 0xAA;

        // Set disk signature
        data[440..444].copy_from_slice(&0x12345678u32.to_le_bytes());

        // Create partition 1: FAT32 LBA, bootable, starts at LBA 2048, size 100MB
        let partition1_offset = 446;
        data[partition1_offset] = 0x80; // bootable
        data[partition1_offset + 4] = 0x0C; // FAT32 LBA
        data[partition1_offset + 8..partition1_offset + 12].copy_from_slice(&2048u32.to_le_bytes());
        data[partition1_offset + 12..partition1_offset + 16]
            .copy_from_slice(&204800u32.to_le_bytes());

        // Create partition 2: Linux, starts after partition 1
        let partition2_offset = 446 + 16;
        data[partition2_offset + 4] = 0x83; // Linux
        data[partition2_offset + 8..partition2_offset + 12]
            .copy_from_slice(&206848u32.to_le_bytes());
        data[partition2_offset + 12..partition2_offset + 16]
            .copy_from_slice(&102400u32.to_le_bytes());

        data
    }

    #[test]
    fn test_mbr_new() {
        let mbr = super::Mbr::new();
        assert!(mbr.is_valid());
        assert_eq!(mbr.get_disk_signature(), 0);
        assert_eq!(mbr.get_partition_count(), 0);
        assert!(!mbr.has_gpt_protective_partition());
        assert!(!mbr.has_overlapping_partitions());
    }

    #[test]
    fn test_mbr_new_with_signature() {
        let signature = 0xDEADBEEF;
        let mbr = Mbr::new_with_signature(signature);
        assert!(mbr.is_valid());
        assert_eq!(mbr.get_disk_signature(), signature);
    }

    #[test]
    fn test_mbr_from_bytes() {
        let data = create_sample_mbr_bytes();
        let mbr = Mbr::from_bytes(&data).expect("Should parse MBR successfully");

        assert!(mbr.is_valid());
        assert_eq!(mbr.get_disk_signature(), 0x12345678);
        assert_eq!(mbr.get_partition_count(), 2);

        let partitions = mbr.get_valid_partitions();
        assert_eq!(partitions.len(), 2);

        // Check first partition
        let p1 = &partitions[0];
        assert!(p1.bootable);
        assert_eq!(p1.kind, super::PartitionKind::Fat32Lba);
        assert_eq!(p1.start_block, 2048);
        assert_eq!(p1.block_count, 204800);

        // Check second partition
        let p2 = &partitions[1];
        assert!(!p2.bootable);
        assert_eq!(p2.kind, super::PartitionKind::Linux);
        assert_eq!(p2.start_block, 206848);
        assert_eq!(p2.block_count, 102400);
    }

    #[test]
    fn test_mbr_from_bytes_invalid_signature() {
        let mut data = create_sample_mbr_bytes();
        data[510] = 0x00; // Invalid signature
        data[511] = 0x00;

        let result = Mbr::from_bytes(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::InvalidSignature);
    }

    #[test]
    fn test_mbr_from_bytes_too_small() {
        let data = [0u8; 256]; // Too small
        let result = Mbr::from_bytes(&data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::BufferTooSmall);
    }

    #[test]
    fn test_mbr_to_bytes_round_trip() {
        let original_data = create_sample_mbr_bytes();
        let mbr = Mbr::from_bytes(&original_data).unwrap();
        let serialized_data = mbr.to_bytes();

        assert_eq!(original_data.len(), serialized_data.len());
        assert_eq!(original_data, serialized_data);
    }

    #[test]
    fn test_mbr_disk_signature() {
        let mut mbr = super::Mbr::new();
        assert_eq!(mbr.get_disk_signature(), 0);

        mbr.set_disk_signature(0xCAFEBABE);
        assert_eq!(mbr.get_disk_signature(), 0xCAFEBABE);
    }

    #[test]
    fn test_mbr_get_bootable_partition() {
        let data = create_sample_mbr_bytes();
        let mbr = Mbr::from_bytes(&data).unwrap();

        let bootable = mbr.get_bootable_partition();
        assert!(bootable.is_some());
        assert_eq!(bootable.unwrap().kind, super::PartitionKind::Fat32Lba);
    }

    #[test]
    fn test_mbr_set_bootable_partition() {
        let mut mbr = super::Mbr::new();

        // Add two partitions
        mbr.add_partition(super::PartitionKind::Fat32, 2048, 100000, false)
            .unwrap();
        mbr.add_partition(super::PartitionKind::Linux, 102048, 50000, true)
            .unwrap();

        // Second partition should be bootable
        let bootable = mbr.get_bootable_partition().unwrap();
        assert_eq!(bootable.kind, super::PartitionKind::Linux);

        // Set first partition as bootable
        mbr.set_bootable_partition(0).unwrap();
        let bootable = mbr.get_bootable_partition().unwrap();
        assert_eq!(bootable.kind, super::PartitionKind::Fat32);
    }

    #[test]
    fn test_mbr_add_partition() {
        let mut mbr = super::Mbr::new();

        let index = mbr
            .add_partition(super::PartitionKind::Fat32Lba, 2048, 204800, true)
            .unwrap();

        assert_eq!(index, 0);
        assert_eq!(mbr.get_partition_count(), 1);

        let partition = &mbr.partitions[index];
        assert!(partition.is_valid());
        assert!(partition.bootable);
        assert_eq!(partition.kind, super::PartitionKind::Fat32Lba);
    }

    #[test]
    fn test_mbr_add_overlapping_partition() {
        let mut mbr = super::Mbr::new();

        // Add first partition
        mbr.add_partition(super::PartitionKind::Fat32, 1000, 2000, false)
            .unwrap();

        // Try to add overlapping partition
        let result = mbr.add_partition(super::PartitionKind::Linux, 1500, 1000, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::OverlappingPartitions);
    }

    #[test]
    fn test_mbr_add_too_many_partitions() {
        let mut mbr = super::Mbr::new();

        // Fill all 4 partition slots
        for i in 0..4 {
            let start = (i as u32) * 10000 + 1000;
            mbr.add_partition(super::PartitionKind::Linux, start, 5000, false)
                .unwrap();
        }

        // Try to add fifth partition
        let result = mbr.add_partition(super::PartitionKind::Fat32, 50000, 1000, false);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::Full);
    }

    #[test]
    fn test_mbr_remove_partition() {
        let mut mbr = super::Mbr::new();

        mbr.add_partition(super::PartitionKind::Fat32, 2048, 100000, false)
            .unwrap();
        assert_eq!(mbr.get_partition_count(), 1);

        mbr.remove_partition(0).unwrap();
        assert_eq!(mbr.get_partition_count(), 0);
    }

    #[test]
    fn test_mbr_remove_invalid_partition() {
        let mut mbr = super::Mbr::new();
        let result = mbr.remove_partition(5); // Invalid index
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::InvalidIndex);
    }

    #[test]
    fn test_mbr_free_partition_slot() {
        let mut mbr = super::Mbr::new();

        // Should have first slot free
        assert_eq!(mbr.get_free_partition_slot(), Some(0));

        // Fill first two slots
        mbr.add_partition(super::PartitionKind::Fat32, 1000, 1000, false)
            .unwrap();
        mbr.add_partition(super::PartitionKind::Linux, 3000, 1000, false)
            .unwrap();

        // Should have third slot free
        assert_eq!(mbr.get_free_partition_slot(), Some(2));

        // Fill remaining slots
        mbr.add_partition(super::PartitionKind::LinuxSwap, 5000, 1000, false)
            .unwrap();
        mbr.add_partition(super::PartitionKind::NtfsExfat, 7000, 1000, false)
            .unwrap();

        // Should have no free slots
        assert_eq!(mbr.get_free_partition_slot(), None);
    }

    #[test]
    fn test_mbr_has_gpt_protective() {
        let mut mbr = super::Mbr::new();
        assert!(!mbr.has_gpt_protective_partition());

        mbr.add_partition(super::PartitionKind::GptProtective, 1, 0xFFFFFFFF, false)
            .unwrap();
        assert!(mbr.has_gpt_protective_partition());
    }

    #[test]
    fn test_mbr_overlapping_partitions_detection() {
        let mut mbr = super::Mbr::new();

        // Add non-overlapping partitions
        mbr.add_partition(super::PartitionKind::Fat32, 1000, 1000, false)
            .unwrap();
        mbr.add_partition(super::PartitionKind::Linux, 3000, 1000, false)
            .unwrap();
        assert!(!mbr.has_overlapping_partitions());

        // Manually create overlapping partitions (bypassing validation)
        mbr.partitions[2] =
            PartitionEntry::new_with_params(false, super::PartitionKind::LinuxSwap, 1500, 1000);
        assert!(mbr.has_overlapping_partitions());
    }

    #[test]
    fn test_mbr_default() {
        let mbr = super::Mbr::default();
        assert!(mbr.is_valid());
        assert_eq!(mbr.get_partition_count(), 0);
    }

    #[test]
    fn test_mbr_display() {
        let data = create_sample_mbr_bytes();
        let mbr = Mbr::from_bytes(&data).unwrap();

        let display_string = format!("{mbr}");
        assert!(display_string.contains("Master Boot Record"));
        assert!(display_string.contains("Disk Signature: 0x12345678"));
        assert!(display_string.contains("Valid: true"));
        assert!(display_string.contains("Partition Count: 2"));
        assert!(display_string.contains("FAT32 LBA"));
        assert!(display_string.contains("Linux"));
    }

    #[test]
    fn test_mbr_constants() {
        assert_eq!(super::Mbr::SIZE, 512);
        assert_eq!(super::Mbr::MAXIMUM_PARTITIONS_COUNT, 4);
        assert_eq!(super::Mbr::SIGNATURE, [0x55, 0xAA]);
    }

    #[test]
    fn test_find_or_create_partition_with_signature_existing() {
        let mbr_data = create_sample_mbr_bytes();
        // Create a larger device (4MB) and put the MBR at the beginning
        let mut data = vec![0u8; 4096 * 1024];
        data[0..512].copy_from_slice(&mbr_data);

        let device = MemoryDevice::<512>::from_vec(data);

        // The sample MBR has signature 0x12345678
        let result = Mbr::find_or_create_partition_with_signature(
            &device,
            0x12345678,
            PartitionKind::Fat32Lba,
        );
        assert!(result.is_ok());

        let partition_device = result.unwrap();
        assert!(partition_device.is_valid());
        assert_eq!(partition_device.get_start_lba(), Mbr::MINIMUM_START_BLOCK); // From sample data
    }

    #[test]
    fn test_find_or_create_partition_with_signature_wrong_signature() {
        let mbr_data = create_sample_mbr_bytes();
        // Create a larger device (4MB) and put the MBR at the beginning
        let mut data = vec![0u8; 4096 * 1024];
        data[0..512].copy_from_slice(&mbr_data);

        let device = MemoryDevice::<512>::from_vec(data);

        // Request different signature than what's in the sample MBR
        let result = Mbr::find_or_create_partition_with_signature(
            &device,
            0xABCDEF00, // Different from 0x12345678 in sample
            PartitionKind::Linux,
        );

        assert!(result.is_ok());

        let partition_device = result.unwrap();
        assert!(partition_device.is_valid());

        // Verify new MBR was created with correct signature
        let new_mbr = Mbr::read_from_device(&device).unwrap();
        assert_eq!(new_mbr.get_disk_signature(), 0xABCDEF00);

        // Check partition type
        let valid_partitions = new_mbr.get_valid_partitions();
        assert_eq!(valid_partitions.len(), 1);
        assert_eq!(valid_partitions[0].kind, PartitionKind::Linux);
    }

    #[test]
    fn test_find_or_create_partition_with_signature_no_mbr() {
        // Create device with no MBR
        let data = vec![0u8; 4096 * 1024]; // 8MB device with no MBR
        let device = MemoryDevice::<512>::from_vec(data);

        let result = Mbr::find_or_create_partition_with_signature(
            &device,
            0x11223344,
            PartitionKind::Fat32Lba,
        );
        assert!(result.is_ok());

        let partition_device = result.unwrap();
        assert!(partition_device.is_valid());
        assert_eq!(partition_device.get_start_lba(), Mbr::MINIMUM_START_BLOCK); // Standard alignment

        // Verify MBR was created with correct signature
        let mbr = Mbr::read_from_device(&device).unwrap();
        assert_eq!(mbr.get_disk_signature(), 0x11223344);
        assert!(mbr.is_valid());
    }

    #[test]
    fn test_find_or_create_partition_with_signature_empty_mbr() {
        // Create device with valid MBR but no partitions
        let mut data = vec![0u8; 4096 * 1024];
        let empty_mbr = Mbr::new_with_signature(0x55667788);
        let mbr_bytes = empty_mbr.to_bytes();
        data[0..512].copy_from_slice(&mbr_bytes);

        let device = MemoryDevice::<512>::from_vec(data);

        let result = Mbr::find_or_create_partition_with_signature(
            &device,
            0x55667788, // Same signature as existing MBR
            PartitionKind::NtfsExfat,
        );
        assert!(result.is_ok());

        let partition_device = result.unwrap();
        assert!(partition_device.is_valid());

        // Verify partition was created with correct type
        let mbr = Mbr::read_from_device(&device).unwrap();
        assert_eq!(mbr.get_disk_signature(), 0x55667788);
        let valid_partitions = mbr.get_valid_partitions();
        assert_eq!(valid_partitions.len(), 1);
        assert_eq!(valid_partitions[0].kind, PartitionKind::NtfsExfat);
    }

    #[test]
    fn test_format_disk_with_signature_and_partition() {
        let data = vec![0u8; 2048 * 1024]; // 4MB device
        let device = MemoryDevice::<512>::from_vec(data);

        let result = Mbr::format_disk_with_signature_and_partition(
            &device,
            0x99887766,
            PartitionKind::LinuxSwap,
        );
        assert!(result.is_ok());

        let partition_device: PartitionDevice<'_, MemoryDevice<512>> = result.unwrap();
        assert!(partition_device.is_valid());
        assert_eq!(partition_device.get_start_lba(), Mbr::MINIMUM_START_BLOCK);

        // Verify MBR
        let mbr = Mbr::read_from_device(&device).unwrap();
        assert_eq!(mbr.get_disk_signature(), 0x99887766);
        assert!(mbr.is_valid());

        let valid_partitions = mbr.get_valid_partitions();
        assert_eq!(valid_partitions.len(), 1);
        assert_eq!(valid_partitions[0].kind, PartitionKind::LinuxSwap);
        assert!(valid_partitions[0].bootable); // Should be bootable by default
    }

    #[test]
    fn test_format_disk_with_signature_and_partition_device_too_small() {
        // Create very small device (less than 2048 sectors)
        let data = vec![0u8; 1024]; // 2 sectors only
        let device = MemoryDevice::<512>::from_vec(data);

        let result = Mbr::format_disk_with_signature_and_partition(
            &device,
            0x12345678,
            PartitionKind::Fat32Lba,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), Error::DeviceTooSmall);
    }
}
