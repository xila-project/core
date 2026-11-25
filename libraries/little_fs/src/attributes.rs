use core::fmt::Debug;

use alloc::ffi::CString;

use file_system::{
    AttributeFlags, Attributes, Inode, IoErrorKind, Kind, Permissions, Result, Time,
};
use users::{GroupIdentifier, UserIdentifier};

use crate::{error::convert_result, littlefs};

/// True when the entry exists but carries no Xila attributes: the signature of
/// an entry whose creation was interrupted between the entry commit and the
/// attribute commit. A transient read error can make a lookup miss an existing
/// entry, so existence alone proves nothing; an entry that has attributes is
/// never one a create call half-created, and an unreadable device reports
/// neither, which leaves the entry in place for the caller to handle.
pub(crate) fn entry_is_attribute_less(file_system: &mut littlefs::lfs_t, path: &CString) -> bool {
    let mut buffer = InternalAttributes::new();
    unsafe {
        matches!(
            convert_result(littlefs::lfs_getattr(
                file_system,
                path.as_ptr(),
                InternalAttributes::IDENTIFIER,
                &mut buffer as *mut _ as *mut _,
                size_of::<InternalAttributes>() as u32,
            )),
            Err(IoErrorKind::MissingAttribute)
        )
    }
}

#[derive(Clone)]
#[repr(C)] // For stable layout
pub struct InternalAttributes {
    pub inode: Inode,
    pub creation: Time,
    pub modification: Time,
    pub access: Time,
    pub status: Time,
    pub user: UserIdentifier,
    pub group: GroupIdentifier,
    pub permissions: Permissions,
    pub kind: Kind,
    // Occupies the existing trailing byte: never persist implicit padding.
    reserved: u8,
}

impl Debug for InternalAttributes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InternalAttributes")
            .field("inode", &self.inode)
            //.field("creation", &self.creation)
            //.field("modification", &self.modification)
            //.field("access", &self.access)
            //.field("status", &self.status)
            .field("user", &self.user)
            .field("group", &self.group)
            .field("permissions", &self.permissions)
            .field("kind", &(self.kind as u8))
            .finish()
    }
}

impl InternalAttributes {
    pub const IDENTIFIER: u8 = 1;

    pub const PERSISTED_ATTRIBUTE_FLAGS: AttributeFlags = AttributeFlags::all()
        .difference(AttributeFlags::Size)
        .difference(AttributeFlags::Links);

    pub fn validate_attribute_mask(mask: AttributeFlags) -> Result<()> {
        if mask.difference(Self::PERSISTED_ATTRIBUTE_FLAGS).is_empty() {
            Ok(())
        } else {
            Err(IoErrorKind::Unsupported)
        }
    }

    /// Backend defaults: no owner rights, regular file kind, zero times and
    /// inode. The create path applies the caller's attribute mask on top.
    pub const fn new() -> Self {
        Self {
            inode: 0,
            creation: Time::new(0),
            modification: Time::new(0),
            access: Time::new(0),
            status: Time::new(0),
            user: UserIdentifier::ROOT,
            group: GroupIdentifier::ROOT,
            permissions: Permissions::NONE,
            kind: Kind::File,
            reserved: 0,
        }
    }

    /// Build the stored attributes for a new entry: backend defaults with the
    /// fields selected by `attributes.get_mask()` applied on top. Used on the
    /// create path so the real owner, permissions and times are committed
    /// together with the entry instead of being rewritten afterwards.
    pub fn from_attributes(attributes: &Attributes) -> Self {
        let mut internal = Self::new();
        // Ignore the result: fields absent from the mask keep their defaults,
        // and the mask was already validated by the caller.
        let _ = internal.update_with_attributes(attributes);
        internal
    }

    pub fn update_attributes(&self, statistics: &mut Attributes) -> Result<()> {
        let requested = statistics.get_mask();
        let available = requested.intersection(Self::PERSISTED_ATTRIBUTE_FLAGS);
        *statistics = statistics.clone().set_mask(available);

        if let Some(inode) = statistics.get_mutable_inode() {
            *inode = self.inode;
        }

        if let Some(time) = statistics.get_mutable_access() {
            *time = self.access;
        }

        if let Some(time) = statistics.get_mutable_creation() {
            *time = self.creation;
        }

        if let Some(time) = statistics.get_mutable_modification() {
            *time = self.modification;
        }

        if let Some(time) = statistics.get_mutable_status() {
            *time = self.status;
        }

        if let Some(permissions) = statistics.get_mutable_permissions() {
            *permissions = self.permissions;
        }

        if let Some(user) = statistics.get_mutable_user() {
            *user = self.user;
        }

        if let Some(group) = statistics.get_mutable_group() {
            *group = self.group;
        }

        if let Some(kind) = statistics.get_mutable_kind() {
            *kind = self.kind;
        }

        Ok(())
    }

    pub fn update_with_attributes(&mut self, statistics: &Attributes) -> Result<()> {
        self.reserved = 0;
        if let Some(inode) = statistics.get_inode() {
            self.inode = *inode;
        }

        if let Some(time) = statistics.get_access() {
            self.access = *time;
        }

        if let Some(time) = statistics.get_creation() {
            self.creation = *time;
        }

        if let Some(time) = statistics.get_modification() {
            self.modification = *time;
        }

        if let Some(time) = statistics.get_status() {
            self.status = *time;
        }

        if let Some(permissions) = statistics.get_permissions() {
            self.permissions = *permissions;
        }

        if let Some(user) = statistics.get_user() {
            self.user = *user;
        }

        if let Some(group) = statistics.get_group() {
            self.group = *group;
        }

        if let Some(kind) = statistics.get_kind() {
            self.kind = *kind;
        }

        Ok(())
    }
}
