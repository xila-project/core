use users::{GroupIdentifier, UserIdentifier};

use crate::{Attributes, Kind, Size, Time};

use super::{Inode, Permissions};

/// Statistics of a file.
///
/// This type contains information about a file, such as its size, inode, etc.
///
/// # Fields
///
/// * `File_system`: The file system the file is on.
/// * `Inode`: The inode of the file.
/// * `Links`: The number of hard links to the file.
/// * `Size`: The size of the file.
/// * `Last_access`: The last time the file was accessed.
/// * `Last_modification`: The last time the file was modified.
/// * `Last_status_change`: The last time the file's status was changed.
/// * `Type`: The type of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statistics {
    pub group: GroupIdentifier,
    pub user: UserIdentifier,
    pub inode: Inode,
    pub creation: Time,
    pub access: Time,
    pub modification: Time,
    pub status: Time,
    pub links: Size,
    pub permissions: Permissions,
    pub kind: Kind,
    pub size: Size,
}

impl Statistics {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inode: Inode,
        links: Size,
        size: Size,
        creation: Time,
        access: Time,
        modification: Time,
        status: Time,
        type_value: Kind,
        permissions: Permissions,
        user: UserIdentifier,
        group: GroupIdentifier,
    ) -> Self {
        Statistics {
            inode,
            links,
            size,
            creation,
            access,
            modification,
            status,
            kind: type_value,
            permissions,
            user,
            group,
        }
    }

    pub fn from_attributes(attributes: &Attributes) -> Option<Self> {
        // Backends without hard links report no link-count attribute. A
        // directory's size is also commonly undefined. Keep the aggregate
        // Statistics API usable by assigning conventional values to those
        // two nonessential fields.
        let links = attributes.get_links().copied().unwrap_or(1);
        let size = attributes.get_size().copied().unwrap_or(0);
        Some(Statistics::new(
            *attributes.get_inode()?,
            links,
            size,
            *attributes.get_creation()?,
            *attributes.get_access()?,
            *attributes.get_modification()?,
            *attributes.get_status()?,
            *attributes.get_kind()?,
            *attributes.get_permissions()?,
            *attributes.get_user()?,
            *attributes.get_group()?,
        ))
    }
}
