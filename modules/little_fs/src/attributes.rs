use core::{fmt::Debug, mem::MaybeUninit};

use file_system::{Attributes, Inode, Kind, Permissions, Result, Time};
use users::{GroupIdentifier, UserIdentifier};

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

    pub fn new_uninitialized() -> MaybeUninit<Self> {
        MaybeUninit::new(Self {
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
        })
    }

    pub fn update_attributes(&self, statistics: &mut Attributes) -> Result<()> {
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
