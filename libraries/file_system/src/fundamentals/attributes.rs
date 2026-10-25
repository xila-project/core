use crate::{Inode, Kind, Permissions, Size, Time};
use bitflags::bitflags;
use core::fmt::Debug;
use users::{GroupIdentifier, UserIdentifier};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AttributeFlags: u16 {
        const Inode = 1 << 0;
        const Kind = 1 << 1;
        const Size = 1 << 2;
        const Links = 1 << 3;
        const CreationTime = 1 << 4;
        const ModificationTime = 1 << 5;
        const AccessTime = 1 << 6;
        const StatusTime = 1 << 7;
        const Permissions = 1 << 8;
        const User = 1 << 9;
        const Group = 1 << 10;
    }
}

/// File attributes.
///
/// The attributes are metadata associated with the file that stores:
/// - The file type.
/// - The file creation time.
/// - The file modification time.
/// - The file access time.
/// - The file permissions.
/// - The file owner.
/// - The file group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributes {
    /// The file inode.
    inode: Inode,
    /// The file type.
    kind: Kind,
    /// Size
    size: Size,
    /// Links
    links: Size,
    /// The file status change time.
    status: Time,
    /// The file modification time.
    modification: Time,
    /// The file access time.
    access: Time,
    /// The file creation time.
    creation: Time,
    /// The file permissions.
    permissions: Permissions,
    /// The file owner.
    user: UserIdentifier,
    /// The file group.
    group: GroupIdentifier,
    /// Mask
    mask: AttributeFlags,
}

/// Macro to generate getter, mutable getter, and setter methods for Attributes fields
macro_rules! generate_attribute_accessors {
    ($getter:ident, $mutable_getter:ident, $setter:ident, $field:ident, $field_type:ty, $mask_flag:expr) => {
        pub fn $getter(&self) -> Option<&$field_type> {
            Self::get(&self.$field, self.mask, $mask_flag)
        }

        pub fn $mutable_getter(&mut self) -> Option<&mut $field_type> {
            Self::get_mutable(&mut self.$field, self.mask, $mask_flag)
        }

        pub fn $setter(mut self, $field: $field_type) -> Self {
            self.mask.insert($mask_flag);
            self.$field = $field;

            self
        }
    };
}

impl Default for Attributes {
    fn default() -> Self {
        Self::new()
    }
}

impl Attributes {
    pub const fn new() -> Self {
        Attributes {
            inode: 0,
            kind: Kind::File,
            size: 0,
            links: 0,
            creation: Time::new(0),
            status: Time::new(0),
            modification: Time::new(0),
            access: Time::new(0),
            permissions: Permissions::NONE,
            user: UserIdentifier::ROOT,
            group: GroupIdentifier::ROOT,
            mask: AttributeFlags::empty(),
        }
    }

    fn get<T>(field: &T, instance_mask: AttributeFlags, field_mask: AttributeFlags) -> Option<&T> {
        if instance_mask.contains(field_mask) {
            Some(field)
        } else {
            None
        }
    }

    fn get_mutable<T>(
        field: &mut T,
        instance_mask: AttributeFlags,
        field_mask: AttributeFlags,
    ) -> Option<&mut T> {
        if instance_mask.contains(field_mask) {
            Some(field)
        } else {
            None
        }
    }

    pub fn get_mask(&self) -> AttributeFlags {
        self.mask
    }

    pub fn set_mask(mut self, mask: AttributeFlags) -> Self {
        self.mask = mask;
        self
    }

    generate_attribute_accessors!(
        get_inode,
        get_mutable_inode,
        set_inode,
        inode,
        Inode,
        AttributeFlags::Inode
    );
    generate_attribute_accessors!(
        get_size,
        get_mutable_size,
        set_size,
        size,
        Size,
        AttributeFlags::Size
    );
    generate_attribute_accessors!(
        get_kind,
        get_mutable_kind,
        set_kind,
        kind,
        Kind,
        AttributeFlags::Kind
    );
    generate_attribute_accessors!(
        get_permissions,
        get_mutable_permissions,
        set_permissions,
        permissions,
        Permissions,
        AttributeFlags::Permissions
    );
    generate_attribute_accessors!(
        get_user,
        get_mutable_user,
        set_user,
        user,
        UserIdentifier,
        AttributeFlags::User
    );
    generate_attribute_accessors!(
        get_group,
        get_mutable_group,
        set_group,
        group,
        GroupIdentifier,
        AttributeFlags::Group
    );
    generate_attribute_accessors!(
        get_creation,
        get_mutable_creation,
        set_creation,
        creation,
        Time,
        AttributeFlags::CreationTime
    );
    generate_attribute_accessors!(
        get_modification,
        get_mutable_modification,
        set_modification,
        modification,
        Time,
        AttributeFlags::ModificationTime
    );
    generate_attribute_accessors!(
        get_access,
        get_mutable_access,
        set_access,
        access,
        Time,
        AttributeFlags::AccessTime
    );
    generate_attribute_accessors!(
        get_status,
        get_mutable_status,
        set_status,
        status,
        Time,
        AttributeFlags::StatusTime
    );
    generate_attribute_accessors!(
        get_links,
        get_mutable_links,
        set_links,
        links,
        Size,
        AttributeFlags::Links
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn test_attributes_new() {
        let attrs = Attributes::new();
        assert_eq!(attrs.inode, 0);
        assert_eq!(attrs.kind, Kind::File);
        assert_eq!(attrs.size, 0);
        assert_eq!(attrs.links, 0);
        assert_eq!(attrs.permissions, Permissions::NONE);
        assert_eq!(attrs.user, UserIdentifier::ROOT);
        assert_eq!(attrs.group, GroupIdentifier::ROOT);
        assert_eq!(attrs.mask, AttributeFlags::empty());
    }

    #[test]
    fn test_attributes_default() {
        let attrs = Attributes::default();
        assert_eq!(attrs, Attributes::new());
    }

    #[test]
    fn test_attributes_set_and_get_inode() {
        let attrs = Attributes::new().set_inode(42);
        assert_eq!(attrs.get_inode(), Some(&42));
        assert!(attrs.mask.contains(AttributeFlags::Inode));
    }

    #[test]
    fn test_attributes_set_and_get_size() {
        let attrs = Attributes::new().set_size(1024);
        assert_eq!(attrs.get_size(), Some(&1024));
        assert!(attrs.mask.contains(AttributeFlags::Size));
    }

    #[test]
    fn test_attributes_set_and_get_kind() {
        let attrs = Attributes::new().set_kind(Kind::Directory);
        assert_eq!(attrs.get_kind(), Some(&Kind::Directory));
        assert!(attrs.mask.contains(AttributeFlags::Kind));
    }

    #[test]
    fn test_attributes_set_and_get_permissions() {
        let perms = Permissions::USER_READ_WRITE;
        let attrs = Attributes::new().set_permissions(perms);
        assert_eq!(attrs.get_permissions(), Some(&perms));
        assert!(attrs.mask.contains(AttributeFlags::Permissions));
    }

    #[test]
    fn test_attributes_set_and_get_user() {
        let user = UserIdentifier::new(1000);
        let attrs = Attributes::new().set_user(user);
        assert_eq!(attrs.get_user(), Some(&user));
        assert!(attrs.mask.contains(AttributeFlags::User));
    }

    #[test]
    fn test_attributes_set_and_get_group() {
        let group = GroupIdentifier::new(1000);
        let attrs = Attributes::new().set_group(group);
        assert_eq!(attrs.get_group(), Some(&group));
        assert!(attrs.mask.contains(AttributeFlags::Group));
    }

    #[test]
    fn test_attributes_set_and_get_creation_time() {
        let time = Time::new(123456);
        let attrs = Attributes::new().set_creation(time);
        assert_eq!(attrs.get_creation(), Some(&time));
        assert!(attrs.mask.contains(AttributeFlags::CreationTime));
    }

    #[test]
    fn test_attributes_set_and_get_modification_time() {
        let time = Time::new(123456);
        let attrs = Attributes::new().set_modification(time);
        assert_eq!(attrs.get_modification(), Some(&time));
        assert!(attrs.mask.contains(AttributeFlags::ModificationTime));
    }

    #[test]
    fn test_attributes_set_and_get_access_time() {
        let time = Time::new(123456);
        let attrs = Attributes::new().set_access(time);
        assert_eq!(attrs.get_access(), Some(&time));
        assert!(attrs.mask.contains(AttributeFlags::AccessTime));
    }

    #[test]
    fn test_attributes_set_and_get_status_time() {
        let time = Time::new(123456);
        let attrs = Attributes::new().set_status(time);
        assert_eq!(attrs.get_status(), Some(&time));
        assert!(attrs.mask.contains(AttributeFlags::StatusTime));
    }

    #[test]
    fn test_attributes_set_and_get_links() {
        let attrs = Attributes::new().set_links(5);
        assert_eq!(attrs.get_links(), Some(&5));
        assert!(attrs.mask.contains(AttributeFlags::Links));
    }

    #[test]
    fn test_attributes_get_returns_none_when_not_set() {
        let attrs = Attributes::new();
        assert_eq!(attrs.get_inode(), None);
        assert_eq!(attrs.get_size(), None);
        assert_eq!(attrs.get_kind(), None);
        assert_eq!(attrs.get_permissions(), None);
        assert_eq!(attrs.get_user(), None);
        assert_eq!(attrs.get_group(), None);
        assert_eq!(attrs.get_creation(), None);
        assert_eq!(attrs.get_modification(), None);
        assert_eq!(attrs.get_access(), None);
        assert_eq!(attrs.get_status(), None);
        assert_eq!(attrs.get_links(), None);
    }

    #[test]
    fn test_attributes_mutable_getter() {
        let mut attrs = Attributes::new().set_inode(42);

        if let Some(inode) = attrs.get_mutable_inode() {
            *inode = 100;
        }

        assert_eq!(attrs.get_inode(), Some(&100));
    }

    #[test]
    fn test_attributes_mutable_getter_returns_none() {
        let mut attrs = Attributes::new();
        assert!(attrs.get_mutable_inode().is_none());
    }

    #[test]
    fn test_attributes_set_mask() {
        let mask = AttributeFlags::Inode | AttributeFlags::Size;
        let attrs = Attributes::new().set_mask(mask);
        assert_eq!(attrs.get_mask(), mask);
    }

    #[test]
    fn test_attributes_chaining_setters() {
        let attrs = Attributes::new()
            .set_inode(42)
            .set_size(1024)
            .set_kind(Kind::Directory)
            .set_permissions(Permissions::USER_FULL);

        assert_eq!(attrs.get_inode(), Some(&42));
        assert_eq!(attrs.get_size(), Some(&1024));
        assert_eq!(attrs.get_kind(), Some(&Kind::Directory));
        assert_eq!(attrs.get_permissions(), Some(&Permissions::USER_FULL));

        assert!(attrs.mask.contains(AttributeFlags::Inode));
        assert!(attrs.mask.contains(AttributeFlags::Size));
        assert!(attrs.mask.contains(AttributeFlags::Kind));
        assert!(attrs.mask.contains(AttributeFlags::Permissions));
    }

    #[test]
    fn test_attributes_clone() {
        let attrs1 = Attributes::new().set_inode(42).set_size(1024);
        let attrs2 = attrs1.clone();

        assert_eq!(attrs1, attrs2);
        assert_eq!(attrs2.get_inode(), Some(&42));
        assert_eq!(attrs2.get_size(), Some(&1024));
    }

    #[test]
    fn test_attributes_partial_eq() {
        let attrs1 = Attributes::new().set_inode(42);
        let attrs2 = Attributes::new().set_inode(42);
        let attrs3 = Attributes::new().set_inode(43);

        assert_eq!(attrs1, attrs2);
        assert_ne!(attrs1, attrs3);
    }

    #[test]
    fn test_attribute_flags_bit_values_and_unknown_bits() {
        assert_eq!(AttributeFlags::Inode.bits(), 1 << 0);
        assert_eq!(AttributeFlags::Kind.bits(), 1 << 1);
        assert_eq!(AttributeFlags::Size.bits(), 1 << 2);
        assert_eq!(AttributeFlags::Links.bits(), 1 << 3);
        assert_eq!(AttributeFlags::CreationTime.bits(), 1 << 4);
        assert_eq!(AttributeFlags::ModificationTime.bits(), 1 << 5);
        assert_eq!(AttributeFlags::AccessTime.bits(), 1 << 6);
        assert_eq!(AttributeFlags::StatusTime.bits(), 1 << 7);
        assert_eq!(AttributeFlags::Permissions.bits(), 1 << 8);
        assert_eq!(AttributeFlags::User.bits(), 1 << 9);
        assert_eq!(AttributeFlags::Group.bits(), 1 << 10);
        assert_eq!(AttributeFlags::all().bits(), (1 << 11) - 1);

        assert_eq!(AttributeFlags::from_bits(1 << 11), None);
        assert_eq!(
            AttributeFlags::from_bits_truncate((1 << 12) - 1),
            AttributeFlags::all()
        );
        assert_eq!(AttributeFlags::from_bits_retain(1 << 11).bits(), 1 << 11);
    }
}
