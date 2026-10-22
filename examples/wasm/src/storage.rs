use drivers_wasm::devices::DriveDevice;
use xila::{authentication, file_system, little_fs};

/// Seeded and existing disks are already formatted. Errors never authorize erasure.
pub fn mount(drive: &'static DriveDevice) -> file_system::Result<little_fs::FileSystem> {
    little_fs::mount_disk(drive, 256)
}

pub async fn initialize_users(
    context: &authentication::Context<'_>,
    _new_disk: bool,
) -> authentication::Result<()> {
    authentication::initialize_seeded(context).await
}
