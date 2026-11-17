use file_system::{AttributeFlags, Attributes, Error, Inode, Kind, Permissions, Result, Time};
use users::{GroupIdentifier, UserIdentifier};

/// All attribute fields stored in RAM per node.
///
/// [`AttributeFlags::Size`] and [`AttributeFlags::Links`] are intentionally excluded:
/// size is reported as `0` for regular files and symbolic links when requested, and
/// links are omitted so the virtual file system defaults them to `1`.
pub(crate) const SUPPORTED_ATTRIBUTE_FLAGS: AttributeFlags = AttributeFlags::all()
    .difference(AttributeFlags::Size)
    .difference(AttributeFlags::Links);

pub(crate) fn validate_attribute_mask(mask: AttributeFlags) -> Result<()> {
    if mask.difference(SUPPORTED_ATTRIBUTE_FLAGS).is_empty() {
        Ok(())
    } else {
        Err(Error::UnsupportedOperation)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeAttributes {
    pub(crate) inode: Inode,
    pub(crate) creation: Time,
    pub(crate) modification: Time,
    pub(crate) access: Time,
    pub(crate) status: Time,
    pub(crate) user: UserIdentifier,
    pub(crate) group: GroupIdentifier,
    pub(crate) permissions: Permissions,
    pub(crate) kind: Kind,
}

impl NodeAttributes {
    pub(crate) const fn new(kind: Kind, permissions: Permissions) -> Self {
        Self {
            inode: 0,
            creation: Time::new(0),
            modification: Time::new(0),
            access: Time::new(0),
            status: Time::new(0),
            user: UserIdentifier::ROOT,
            group: GroupIdentifier::ROOT,
            permissions,
            kind,
        }
    }

    pub(crate) fn read_into(&self, attributes: &mut Attributes) {
        let requested = attributes.get_mask();
        let mut available = requested.intersection(SUPPORTED_ATTRIBUTE_FLAGS);

        if requested.contains(AttributeFlags::Size)
            && matches!(self.kind, Kind::File | Kind::SymbolicLink)
        {
            available.insert(AttributeFlags::Size);
        }

        *attributes = attributes.clone().set_mask(available);

        if let Some(inode) = attributes.get_mutable_inode() {
            *inode = self.inode;
        }
        if let Some(access) = attributes.get_mutable_access() {
            *access = self.access;
        }
        if let Some(creation) = attributes.get_mutable_creation() {
            *creation = self.creation;
        }
        if let Some(modification) = attributes.get_mutable_modification() {
            *modification = self.modification;
        }
        if let Some(status) = attributes.get_mutable_status() {
            *status = self.status;
        }
        if let Some(permissions) = attributes.get_mutable_permissions() {
            *permissions = self.permissions;
        }
        if let Some(user) = attributes.get_mutable_user() {
            *user = self.user;
        }
        if let Some(group) = attributes.get_mutable_group() {
            *group = self.group;
        }
        if let Some(kind) = attributes.get_mutable_kind() {
            *kind = self.kind;
        }
        if let Some(size) = attributes.get_mutable_size() {
            *size = 0;
        }
    }

    pub(crate) fn apply_from(&mut self, attributes: &Attributes) -> Result<()> {
        let mask = attributes.get_mask();
        if mask.is_empty() {
            return Ok(());
        }

        validate_attribute_mask(mask)?;

        if let Some(kind) = attributes.get_kind() {
            if self.kind == Kind::Directory && *kind != Kind::Directory {
                return Err(Error::IsDirectory);
            }
            if self.kind != Kind::Directory && *kind == Kind::Directory {
                return Err(Error::InvalidParameter);
            }
        }

        if let Some(inode) = attributes.get_inode() {
            self.inode = *inode;
        }
        if let Some(access) = attributes.get_access() {
            self.access = *access;
        }
        if let Some(creation) = attributes.get_creation() {
            self.creation = *creation;
        }
        if let Some(modification) = attributes.get_modification() {
            self.modification = *modification;
        }
        if let Some(status) = attributes.get_status() {
            self.status = *status;
        }
        if let Some(permissions) = attributes.get_permissions() {
            self.permissions = *permissions;
        }
        if let Some(user) = attributes.get_user() {
            self.user = *user;
        }
        if let Some(group) = attributes.get_group() {
            self.group = *group;
        }
        if let Some(kind) = attributes.get_kind() {
            self.kind = *kind;
        }

        Ok(())
    }
}
