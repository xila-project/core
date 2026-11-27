//! Platform-independent Bluetooth Low Energy host service.

#![no_std]

extern crate alloc;

mod host;
mod node;

pub use bt_hci as hci;
pub use device::bluetooth::*;
pub use host::Controller;
pub use node::BluetoothDevice;

use file_system::{IoErrorKind, Path, Permissions, Result};
use task::TaskIdentifier;
use virtual_file_system::{ItemStatic, VirtualFileSystem};

pub const DEVICE_PATH: &str = "/devices/bluetooth/hci0";

/// Mount a controller node owned by the mounting task's user.
/// The runtime must create `/devices` before calling this function.
pub async fn mount(
    file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    device: &'static BluetoothDevice,
) -> Result<()> {
    match file_system
        .create_directory(task, &Path::BLUETOOTH_DEVICES)
        .await
    {
        Ok(()) | Err(IoErrorKind::AlreadyExists) => {}
        Err(error) => return Err(error),
    }

    file_system
        .mount_static_with_permissions(
            task,
            DEVICE_PATH,
            ItemStatic::CharacterDevice(device),
            Permissions::USER_READ_WRITE,
        )
        .await
}
