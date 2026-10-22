use drivers_std::drive_file::FileDriveDevice;
use file_system::{
    DirectBaseOperations, XILA_DISK_SIGNATURE,
    mbr::{Mbr, PartitionKind, create_partition_device},
};
use std::{error::Error, path::Path};

pub fn build(output: &Path, size: u64, root: Option<&Path>) -> Result<(), Box<dyn Error>> {
    if !size.is_multiple_of(512) || size < (Mbr::MINIMUM_START_BLOCK + 2) * 512 {
        return Err(
            "disk size must be a multiple of 512 and accommodate the MBR and LittleFS".into(),
        );
    }
    let source = root.map(super::source::Directory::open).transpose()?;
    if let Some(root) = source.as_ref() {
        super::populate::validate(root)?;
    }
    super::create_output_file(output, size)?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        let name = output.to_str().ok_or("image path must be UTF-8")?;
        {
            let drive = Box::leak(Box::new(FileDriveDevice::new(&name, size)));
            let partition = Mbr::format_disk_with_signature_and_partition(
                drive,
                XILA_DISK_SIGNATURE,
                PartitionKind::Xila,
            )?;
            let fs = little_fs::FileSystem::new_format(Box::leak(Box::new(partition)), 256)?;
            super::populate::populate(&fs, source.as_ref())?;
            drop(fs);
            drive.flush_sync()?;
        }
        {
            let drive = Box::leak(Box::new(FileDriveDevice::new(&name, size)));
            let mbr = Mbr::read_from_device(drive)?;
            mbr.validate()?;
            if mbr.get_disk_signature() != XILA_DISK_SIGNATURE {
                return Err("invalid Xila signature".into());
            }
            let entry = mbr
                .partitions
                .iter()
                .find(|p| p.is_valid() && p.kind == PartitionKind::Xila)
                .ok_or("missing Xila partition")?;
            let partition = create_partition_device(drive, entry)?;
            let fs = little_fs::FileSystem::new(Box::leak(Box::new(partition)), 256)?;
            super::populate::verify(&fs, source.as_ref())?;
            drop(fs);
            drive.flush_sync()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result
}
