use core::fmt::Debug;
use bitflags::bitflags;
use super::Permission;

bitflags! {
    /// File access permissions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AccessFlags: u8 {
        const Read = 1 << 0;
        const Write = 1 << 1;
    }
}

impl AccessFlags {
    pub const READ_WRITE: Self = Self::Read.union(Self::Write);

    pub const fn into_permission(&self) -> Permission {
        let mut permission = Permission::empty();
        if self.contains(Self::Read) {
            permission = permission.union(Permission::Read);
        }
        if self.contains(Self::Write) {
            permission = permission.union(Permission::Write);
        }
        permission
    }
}

bitflags! {
    /// File creation flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CreateFlags: u8 {
        const Create = 1 << 0;
        const Exclusive = 1 << 1;
        const Truncate = 1 << 2;
    }
}

impl CreateFlags {
    pub const CREATE_TRUNCATE: Self = Self::Create.union(Self::Truncate);
    pub const CREATE_EXCLUSIVE: Self = Self::Create.union(Self::Exclusive);
}

bitflags! {
    /// File status flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StateFlags: u8 {
        const Append = 1 << 0;
        const NonBlocking = 1 << 1;
        const Synchronous = 1 << 2;
        const SynchronousDataOnly = 1 << 3;
    }
}

/// All the flags that can be set for a file.
///
/// The flags are stored in a 16-bit integer, with the following layout:
///
/// | Mode | Open | Status |
/// |------|------|--------|
/// | 0-1  | 2-5  | 6-9    |
///
/// # Example
///
/// ```rust
/// use file_system::{Flags, AccessFlags, CreateFlags, StateFlags};
///     
/// let flags = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), Some(StateFlags::NonBlocking));
///
/// assert_eq!(flags.get_access(), AccessFlags::READ_WRITE);
/// assert_eq!(flags.get_create(), CreateFlags::Create);
/// assert_eq!(flags.get_state(), StateFlags::NonBlocking);
/// ```
#[derive(PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct Flags(u16);

impl Debug for Flags {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Flags")
            .field("Access", &self.get_access())
            .field("Create", &self.get_create())
            .field("State", &self.get_state())
            .finish()
    }
}

impl Flags {
    const MODE_POSITION: u8 = 0;
    const OPEN_POSITION: u8 = 2;
    const STATUS_POSITION: u8 = 3 + Self::OPEN_POSITION;

    const OPEN_MASK: u16 = (1 << 3) - 1;
    const STATUS_MASK: u16 = (1 << 4) - 1;
    const MODE_MASK: u16 = (1 << 2) - 1;

    pub const fn new(
        mode: AccessFlags,
        open: Option<CreateFlags>,
        status: Option<StateFlags>,
    ) -> Self {
        let open = if let Some(open_val) = open {
            open_val
        } else {
            CreateFlags::empty()
        };
        let status = if let Some(status_val) = status {
            status_val
        } else {
            StateFlags::empty()
        };

        let mut flags: u16 = 0;
        flags |= (mode.bits() as u16) << Self::MODE_POSITION;
        flags |= (open.bits() as u16) << Self::OPEN_POSITION;
        flags |= (status.bits() as u16) << Self::STATUS_POSITION;
        Self(flags)
    }

    pub const fn get_access(&self) -> AccessFlags {
        AccessFlags::from_bits_retain(((self.0 >> Self::MODE_POSITION) & Self::MODE_MASK) as u8)
    }

    pub const fn get_create(&self) -> CreateFlags {
        CreateFlags::from_bits_retain(((self.0 >> Self::OPEN_POSITION) & Self::OPEN_MASK) as u8)
    }

    pub const fn get_state(&self) -> StateFlags {
        StateFlags::from_bits_retain(((self.0 >> Self::STATUS_POSITION) & Self::STATUS_MASK) as u8)
    }

    pub const fn set_mode(mut self, mode: AccessFlags) -> Self {
        self.0 &= !(Self::MODE_MASK << Self::MODE_POSITION);
        self.0 |= (mode.bits() as u16) << Self::MODE_POSITION;
        self
    }

    pub const fn set_open(mut self, open: CreateFlags) -> Self {
        self.0 &= !(Self::OPEN_MASK << Self::OPEN_POSITION);
        self.0 |= (open.bits() as u16) << Self::OPEN_POSITION;
        self
    }

    pub const fn set_status(mut self, status: StateFlags) -> Self {
        self.0 &= !(Self::STATUS_MASK << Self::STATUS_POSITION);
        self.0 |= (status.bits() as u16) << Self::STATUS_POSITION;
        self
    }

    pub fn is_permission_granted(&self, permission: &Permission) -> bool {
        let mode = self.get_access();

        let read = permission.contains(Permission::Read) && mode.contains(AccessFlags::Read); // Read permission
        let write = permission.contains(Permission::Write)
            && (mode.contains(AccessFlags::Write) || self.get_state().contains(StateFlags::Append)); // Write permission

        read || write
    }

    pub fn split(&self) -> (AccessFlags, CreateFlags, StateFlags) {
        (self.get_access(), self.get_create(), self.get_state())
    }
}

impl From<AccessFlags> for Flags {
    fn from(mode: AccessFlags) -> Self {
        Self::new(mode, None, None)
    }
}

impl From<Flags> for u16 {
    fn from(flags: Flags) -> Self {
        flags.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_type_set_get() {
        let flags = Flags::new(AccessFlags::Read, None, None);

        let new_mode = AccessFlags::Write;
        let flags = flags.set_mode(new_mode);
        assert_eq!(flags.get_access(), new_mode);

        let new_open = CreateFlags::Exclusive | CreateFlags::Truncate;
        let flags = flags.set_open(new_open);
        assert_eq!(flags.get_create(), new_open);

        let new_status = StateFlags::NonBlocking | StateFlags::SynchronousDataOnly;
        let flags = flags.set_status(new_status);
        assert_eq!(flags.get_state(), new_status);
    }

    #[test]
    fn test_flags_type_is_permission_granted() {
        let mode = AccessFlags::READ_WRITE;
        let status = StateFlags::empty();
        let flags = Flags::new(mode, None, Some(status));

        assert!(flags.is_permission_granted(&Permission::Read));
        assert!(flags.is_permission_granted(&Permission::Write));
        assert!(flags.is_permission_granted(&Permission::READ_WRITE));
    }

    #[test]
    fn test_flags_type_from_mode_type() {
        let mode = AccessFlags::READ_WRITE;
        let flags: Flags = mode.into();
        assert_eq!(flags.get_access(), mode);
    }

    #[test]
    fn test_flags_type_into_u16() {
        let mode = AccessFlags::READ_WRITE;
        let flags = Flags::new(mode, None, None);
        let flags_u16: u16 = flags.into();
        assert_eq!(flags_u16, flags.0);
    }
}
