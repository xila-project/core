//! Mount helper for peripheral device nodes.

use file_system::Path;
use task::TaskIdentifier;
use virtual_file_system::{Result, VirtualFileSystem};

use crate::{PeripheralDevice, PeripheralHandle};

/// Mounts `handle` as a character device at `path`, for example
/// `/devices/uart/uart0`. The caller chooses the class directory; this helper
/// adds no permission policy beyond the VFS mount defaults.
pub async fn mount_peripheral<H: PeripheralHandle + 'static>(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: impl AsRef<Path>,
    handle: H,
) -> Result<()> {
    virtual_file_system
        .mount_character_device(task, path, PeripheralDevice::new(handle))
        .await
}
