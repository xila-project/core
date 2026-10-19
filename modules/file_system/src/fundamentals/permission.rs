use core::fmt::{self, Display};

use bitflags::bitflags;

use crate::Kind;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Permission: u8 {
        const Execute = 1 << 0;
        const Write = 1 << 1;
        const Read = 1 << 2;
    }
}

impl Permission {
    pub const READ_WRITE: Self = Permission::Read.union(Permission::Write);
    pub const READ_EXECUTE: Self = Permission::Read.union(Permission::Execute);
    pub const WRITE_EXECUTE: Self = Permission::Write.union(Permission::Execute);

    pub const fn from_unix(unix: u8) -> Option<Self> {
        if unix > 0b111 {
            return None;
        }

        Some(Self::from_bits_retain(unix))
    }

    pub const fn to_unix(&self) -> u8 {
        self.bits()
    }
}

impl Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let read = if self.contains(Permission::Read) {
            "r"
        } else {
            "-"
        };
        let write = if self.contains(Permission::Write) {
            "w"
        } else {
            "-"
        };
        let execute = if self.contains(Permission::Execute) {
            "x"
        } else {
            "-"
        };

        write!(f, "{read}{write}{execute}")
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Special: u8 {
        const Sticky = 1 << 0;
        const SetUserIdentifier = 1 << 1;
        const SetGroupIdentifier = 1 << 2;
    }
}

impl fmt::Display for Special {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let sticky = if self.contains(Special::Sticky) {
            "t"
        } else {
            "-"
        };
        let set_gid = if self.contains(Special::SetGroupIdentifier) {
            "u"
        } else {
            "-"
        };
        let set_uid = if self.contains(Special::SetUserIdentifier) {
            "g"
        } else {
            "-"
        };

        write!(f, "{sticky}{set_gid}{set_uid}")
    }
}

/// Represents the permissions of a file or directory.
///
/// The permissions are divided into three groups: user, group, and others.
/// Each group has three permissions: read, write, and execute.
///
/// # Examples
///
/// ```rust
/// use file_system::{Permissions, Permission, Special};
///
/// let user = Permission::Read; // Read only
/// let group = Permission::Write; // Write only
/// let others = Permission::Execute; // Execute only
/// let special = Special::empty();
///
/// let permissions = Permissions::new(user, group, others, special);
///
/// assert_eq!(permissions.get_user(), user);
/// assert_eq!(permissions.get_group(), group);
/// assert_eq!(permissions.get_others(), others);
/// assert_eq!(permissions.get_special(), special);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Permissions(u16);

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let user = self.get_user();
        let group = self.get_group();
        let others = self.get_others();

        write!(f, "{user}{group}{others}")
    }
}

impl From<(Permission, Permission, Permission)> for Permissions {
    fn from(value: (Permission, Permission, Permission)) -> Self {
        Self::new(value.0, value.1, value.2, Special::NONE)
    }
}

impl From<(Permission, Permission, Permission, Special)> for Permissions {
    fn from(value: (Permission, Permission, Permission, Special)) -> Self {
        Self::new(value.0, value.1, value.2, value.3)
    }
}

impl Permissions {
    pub const NONE: Self = Self::new(
        Permission::empty(),
        Permission::empty(),
        Permission::empty(),
        Special::NONE,
    );
    pub const ALL_FULL: Self = Self::new(
        Permission::all(),
        Permission::all(),
        Permission::all(),
        Special::NONE,
    );
    pub const ALL_READ_WRITE: Self = Self::new(
        Permission::READ_WRITE,
        Permission::READ_WRITE,
        Permission::READ_WRITE,
        Special::NONE,
    );
    pub const USER_FULL: Self = Self::new(
        Permission::all(),
        Permission::empty(),
        Permission::empty(),
        Special::NONE,
    );
    pub const USER_READ_WRITE: Self = Self::new(
        Permission::READ_WRITE,
        Permission::empty(),
        Permission::empty(),
        Special::NONE,
    );
    pub const EXECUTABLE: Self = Self::new(
        Permission::all(),
        Permission::READ_EXECUTE,
        Permission::READ_EXECUTE,
        Special::NONE,
    );
    pub const FILE_DEFAULT: Self = Self::new(
        Permission::READ_WRITE,
        Permission::READ_WRITE,
        Permission::Read,
        Special::NONE,
    );
    pub const DIRECTORY_DEFAULT: Self = Self::new(
        Permission::all(),
        Permission::all(),
        Permission::READ_EXECUTE,
        Special::NONE,
    );
    pub const DEVICE_DEFAULT: Self = Self::new(
        Permission::all(),
        Permission::READ_WRITE,
        Permission::READ_WRITE,
        Special::NONE,
    );

    /// Creates a new permission.
    pub const fn new(
        user: Permission,
        group: Permission,
        others: Permission,
        special: Special,
    ) -> Self {
        Self(
            (special.to_unix() as u16) << 9
                | (user.to_unix() as u16) << 6
                | (group.to_unix() as u16) << 3
                | others.to_unix() as u16,
        )
    }

    /// Creates a new permission with read access for user. No access for group and others.
    pub const fn new_default(r#type: Kind) -> Self {
        match r#type {
            Kind::Directory => Self::new(
                Permission::all(),
                Permission::READ_EXECUTE,
                Permission::READ_EXECUTE,
                Special::NONE,
            ),
            Kind::File => Self::new(
                Permission::READ_WRITE,
                Permission::Read,
                Permission::Read,
                Special::NONE,
            ),
            Kind::Pipe => Self::new(
                Permission::READ_WRITE,
                Permission::empty(),
                Permission::empty(),
                Special::NONE,
            ),
            Kind::BlockDevice => Self::new(
                Permission::all(),
                Permission::READ_WRITE,
                Permission::READ_WRITE,
                Special::NONE,
            ),
            Kind::CharacterDevice => Self::new(
                Permission::READ_WRITE,
                Permission::READ_WRITE,
                Permission::empty(),
                Special::NONE,
            ),
            Kind::Socket => Self::ALL_READ_WRITE,
            Kind::SymbolicLink => Self::ALL_FULL,
        }
    }

    /// Sets the permission for the user.
    pub fn set_user(mut self, user: Permission) -> Self {
        self.0 = (self.0 & 0o7077) | (user.to_unix() as u16) << 6;
        self
    }

    /// Sets the permission for the group.
    pub fn set_group(mut self, group: Permission) -> Self {
        self.0 = (self.0 & 0o7707) | (group.to_unix() as u16) << 3;
        self
    }

    /// Sets the permission for others.
    pub fn set_others(mut self, others: Permission) -> Self {
        self.0 = (self.0 & 0o7770) | others.to_unix() as u16;
        self
    }

    /// Sets the special permissions.
    pub fn set_special(mut self, special: Special) -> Self {
        self.0 = (self.0 & 0o0777) | (special.to_unix() as u16) << 9;
        self
    }

    /// Gets the permission for the user.
    pub fn get_user(&self) -> Permission {
        Permission::from_unix(((self.0 >> 6) & 0b111) as u8).unwrap()
    }

    /// Gets the permission for the group.
    pub fn get_group(&self) -> Permission {
        Permission::from_unix(((self.0 >> 3) & 0b111) as u8).unwrap()
    }

    /// Gets the permission for others.
    pub fn get_others(&self) -> Permission {
        Permission::from_unix((self.0 & 0b111) as u8).unwrap()
    }

    /// Gets the special permissions.
    pub fn get_special(&self) -> Special {
        Special::from_unix((self.0 >> 9) as u8).unwrap()
    }

    /// Converts the permission to a Unix permission.
    pub const fn from_octal(unix: u16) -> Option<Self> {
        if unix > 0o777 {
            return None;
        }

        Some(Self::from_bits_retain(unix))
    }

    /// Converts the permission to a Unix permission.
    pub const fn as_u16(&self) -> u16 {
        self.bits()
    }
}

impl Special {
    pub const NONE: Self = Self::empty();
    pub const STICKY: Self = Self::from_bits_retain(1);
    pub const SET_USER_IDENTIFIER: Self = Self::from_bits_retain(2);
    pub const SET_GROUP_IDENTIFIER: Self = Self::from_bits_retain(4);

    pub fn new(sticky: bool, set_gid: bool, set_uid: bool) -> Self {
        Self::from_bits_retain((sticky as u8) | (set_gid as u8) << 1 | (set_uid as u8) << 2)
    }

    pub fn set_sticky(mut self, sticky: bool) -> Self {
        self = Self::from_bits_retain((self.bits() & 0b110) | sticky as u8);
        self
    }

    pub fn set_set_group_identifier(mut self, set_gid: bool) -> Self {
        self = Self::from_bits_retain((self.bits() & 0b101) | (set_gid as u8) << 1);
        self
    }

    pub fn set_set_user_identifier(mut self, set_uid: bool) -> Self {
        self = Self::from_bits_retain((self.bits() & 0b011) | (set_uid as u8) << 2);
        self
    }

    pub const fn get_sticky(&self) -> bool {
        self.bits() & 0b001 != 0
    }

    pub const fn get_set_group_identifier(&self) -> bool {
        self.bits() & 0b010 != 0
    }

    pub const fn get_set_user_identifier(&self) -> bool {
        self.bits() & 0b100 != 0
    }

    pub const fn to_unix(&self) -> u8 {
        self.bits()
    }

    pub fn from_unix(unix: u8) -> Option<Self> {
        if unix > 0b111 {
            return None;
        }

        Some(Self::from_bits_retain(unix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_permissions() {
        let user = Permission::Read; // Read only
        let group = Permission::Write; // Write only
        let others = Permission::Execute; // Execute only
        let special = Special::new(true, false, true); // Sticky and set user identifier
        let permissions = Permissions::new(user, group, others, special);
        assert_eq!(permissions.0, 0b101_100_010_001);
    }

    #[test]
    fn test_new_permission() {
        assert_eq!(Permission::Read.bits(), 0b100);
        assert_eq!(Permission::Write.bits(), 0b010);
        assert_eq!(Permission::Execute.bits(), 0b001);
        assert_eq!(Permission::READ_WRITE.bits(), 0b110);
        assert_eq!(Permission::WRITE_EXECUTE.bits(), 0b011);
        assert_eq!(Permission::READ_EXECUTE.bits(), 0b101);
        assert_eq!(Permission::empty().0, 0b000);
        assert_eq!(Permission::all().0, 0b111);
    }

    #[test]
    fn test_permission_type_to_unix() {
        let read = Permission::Read;
        assert_eq!(read.to_unix(), 4);
        let write = Permission::Write;
        assert_eq!(write.to_unix(), 2);
        let execute = Permission::Execute;
        assert_eq!(execute.to_unix(), 1);
        let full = Permission::all();
        assert_eq!(full.to_unix(), 7);
        let none = Permission::empty();
        assert_eq!(none.to_unix(), 0);
    }

    #[test]
    fn test_permission_type_from_unix() {
        let read = Permission::from_unix(4).unwrap();
        assert_eq!(read, Permission::Read);
        let write = Permission::from_unix(2).unwrap();
        assert_eq!(write, Permission::Write);
        let execute = Permission::from_unix(1).unwrap();
        assert_eq!(execute, Permission::Execute);
        let full = Permission::from_unix(7).unwrap();
        assert_eq!(full, Permission::all());
        let no = Permission::from_unix(0).unwrap();
        assert_eq!(no, Permission::empty());
    }

    #[test]
    fn test_permissions_type_from_unix() {
        let permissions = Permissions::from_octal(0b101_101_101).unwrap();
        assert_eq!(permissions.get_user().to_unix(), 5);
        assert_eq!(permissions.get_group().to_unix(), 5);
        assert_eq!(permissions.get_others().to_unix(), 5);
    }

    #[test]
    fn test_permissions_type_to_unix() {
        let user = Permission::Read | Permission::Execute; // Read and execute
        let group = Permission::Read | Permission::Write; // Read and write
        let others = Permission::Write | Permission::Execute; // Write and execute
        let special = Special::new(true, false, true); // Sticky and set user identifier
        let permissions = Permissions::new(user, group, others, special);
        assert_eq!(permissions.as_u16(), 0b101_101_110_011);
    }

    #[test]
    fn test_permission_type_contains() {
        let read = Permission::Read;
        let write = Permission::Write;
        let read_write = Permission::READ_WRITE;
        let read_execute = Permission::READ_EXECUTE;
        let write_execute = Permission::WRITE_EXECUTE;
        let execute = Permission::Execute;
        let full = Permission::all();
        let no = Permission::empty();

        assert!(full.contains(read));
        assert!(full.contains(write));
        assert!(full.contains(execute));
        assert!(full.contains(read_write));
        assert!(full.contains(read_execute));
        assert!(full.contains(write_execute));
        assert!(full.contains(full));
        assert!(full.contains(no));

        assert!(read.contains(read));
        assert!(!read.contains(write));
        assert!(!read.contains(execute));
        assert!(!read.contains(read_write));
        assert!(!read.contains(read_execute));
        assert!(!read.contains(write_execute));
        assert!(!read.contains(full));
        assert!(read.contains(no));

        assert!(!write.contains(read));
        assert!(write.contains(write));
        assert!(!write.contains(execute));
        assert!(!write.contains(read_write));
        assert!(!write.contains(read_execute));
        assert!(!write.contains(write_execute));
        assert!(!write.contains(full));
        assert!(write.contains(no));

        assert!(!execute.contains(read));
        assert!(!execute.contains(write));
        assert!(execute.contains(execute));
        assert!(!execute.contains(read_write));
        assert!(!execute.contains(read_execute));
        assert!(!execute.contains(write_execute));
        assert!(!execute.contains(full));
        assert!(execute.contains(no));

        assert!(read_write.contains(read));
        assert!(read_write.contains(write));
        assert!(!read_write.contains(execute));
        assert!(read_write.contains(read_write));
        assert!(!read_write.contains(read_execute));
        assert!(!read_write.contains(write_execute));
        assert!(!read_write.contains(full));
        assert!(read_write.contains(no));

        assert!(read_execute.contains(read));
        assert!(!read_execute.contains(write));
        assert!(read_execute.contains(execute));
        assert!(!read_execute.contains(read_write));
        assert!(read_execute.contains(read_execute));
        assert!(!read_execute.contains(write_execute));
        assert!(!read_execute.contains(full));
        assert!(read_execute.contains(no));

        assert!(!write_execute.contains(read));
        assert!(write_execute.contains(write));
        assert!(write_execute.contains(execute));
        assert!(!write_execute.contains(read_write));
        assert!(!write_execute.contains(read_execute));
        assert!(write_execute.contains(write_execute));
        assert!(!write_execute.contains(full));
        assert!(write_execute.contains(no));

        assert!(!no.contains(read));
        assert!(!no.contains(write));
        assert!(!no.contains(execute));
        assert!(!no.contains(read_write));
        assert!(!no.contains(read_execute));
        assert!(!no.contains(write_execute));
        assert!(!no.contains(full));
        assert!(no.contains(no));
    }
}
