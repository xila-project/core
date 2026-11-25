//! Mount helper for peripheral device nodes.

use alloc::boxed::Box;
use file_system::{Path, Permissions};
use task::TaskIdentifier;
use virtual_file_system::{ItemStatic, Result, VirtualFileSystem};

use crate::{PeripheralDevice, PeripheralHandle};

/// Mounts `handle` as a character device at `path`, for example
/// `/devices/uart/uart0`, with the default device permissions. The caller
/// chooses the class directory; this helper adds no permission policy beyond
/// the VFS mount defaults.
pub async fn mount_peripheral<H: PeripheralHandle + 'static>(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: impl AsRef<Path>,
    handle: H,
) -> Result<()> {
    mount_peripheral_with_permissions(
        virtual_file_system,
        task,
        path,
        handle,
        Permissions::DEVICE_DEFAULT,
    )
    .await
}

/// Mounts `handle` as a character device at `path` with explicit `permissions`,
/// for device classes whose writes are reserved to the mounting user (for
/// example the real-time clock with [`Permissions::PRIVILEGED_DEVICE`]).
pub async fn mount_peripheral_with_permissions<H: PeripheralHandle + 'static>(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: impl AsRef<Path>,
    handle: H,
    permissions: Permissions,
) -> Result<()> {
    virtual_file_system
        .mount_static_with_permissions(
            task,
            path,
            ItemStatic::CharacterDevice(Box::leak(Box::new(PeripheralDevice::new(handle)))),
            permissions,
        )
        .await
}
