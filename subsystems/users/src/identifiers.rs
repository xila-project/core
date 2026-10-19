use core::ops::{Add, AddAssign};

pub type UserIdentifierInner = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct UserIdentifier(UserIdentifierInner);

impl UserIdentifier {
    pub const ROOT: Self = Self::new(0);

    pub const MINIMUM: Self = Self::new(1);
    pub const MAXIMUM: Self = Self::new(UserIdentifierInner::MAX);

    pub const fn new(identifier: UserIdentifierInner) -> Self {
        Self(identifier)
    }

    pub const fn as_u16(self) -> UserIdentifierInner {
        self.0
    }
}

impl AddAssign<UserIdentifierInner> for UserIdentifier {
    fn add_assign(&mut self, other: UserIdentifierInner) {
        self.0 += other;
    }
}

impl Add<UserIdentifierInner> for UserIdentifier {
    type Output = Self;

    fn add(self, other: UserIdentifierInner) -> Self {
        Self::new(self.0 + other)
    }
}

impl From<UserIdentifierInner> for UserIdentifier {
    fn from(value: UserIdentifierInner) -> Self {
        UserIdentifier::new(value)
    }
}
impl From<UserIdentifier> for UserIdentifierInner {
    fn from(value: UserIdentifier) -> Self {
        value.as_u16()
    }
}

pub type GroupIdentifierInner = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct GroupIdentifier(GroupIdentifierInner);

impl GroupIdentifier {
    pub const ROOT: Self = Self::new(0);

    pub const MINIMUM: Self = Self::new(1);
    pub const MAXIMUM: Self = Self::new(GroupIdentifierInner::MAX);

    pub const fn new(identifier: GroupIdentifierInner) -> Self {
        Self(identifier)
    }

    pub const fn as_u16(self) -> GroupIdentifierInner {
        self.0
    }
}

impl From<GroupIdentifierInner> for GroupIdentifier {
    fn from(value: GroupIdentifierInner) -> Self {
        GroupIdentifier::new(value)
    }
}
impl From<GroupIdentifier> for GroupIdentifierInner {
    fn from(value: GroupIdentifier) -> Self {
        value.as_u16()
    }
}

impl AddAssign<GroupIdentifierInner> for GroupIdentifier {
    fn add_assign(&mut self, other: GroupIdentifierInner) {
        self.0 += other;
    }
}

impl Add<GroupIdentifierInner> for GroupIdentifier {
    type Output = Self;

    fn add(self, other: GroupIdentifierInner) -> Self {
        Self::new(self.0 + other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloc::{collections::BTreeMap, format};

    #[test]
    fn test_user_identifier_constants() {
        assert_eq!(UserIdentifier::ROOT.as_u16(), 0);
        assert_eq!(UserIdentifier::MINIMUM.as_u16(), 1);
        assert_eq!(UserIdentifier::MAXIMUM.as_u16(), u16::MAX);
    }

    #[test]
    fn test_user_identifier_new() {
        let id = UserIdentifier::new(42);
        assert_eq!(id.as_u16(), 42);
    }

    #[test]
    fn test_user_identifier_as_u16() {
        let id = UserIdentifier::new(123);
        assert_eq!(id.as_u16(), 123);
    }

    #[test]
    fn test_user_identifier_add_assign() {
        let mut id = UserIdentifier::new(10);
        id += 5;
        assert_eq!(id.as_u16(), 15);
    }

    #[test]
    fn test_user_identifier_add() {
        let id1 = UserIdentifier::new(10);
        let id2 = id1 + 5;
        assert_eq!(id1.as_u16(), 10); // Original unchanged
        assert_eq!(id2.as_u16(), 15);
    }

    #[test]
    fn test_user_identifier_from_u16() {
        let id: UserIdentifier = 42u16.into();
        assert_eq!(id.as_u16(), 42);
    }

    #[test]
    fn test_user_identifier_to_u16() {
        let id = UserIdentifier::new(42);
        let value: u16 = id.into();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_user_identifier_clone_copy() {
        let id1 = UserIdentifier::new(42);
        let id2 = id1; // Copy
        let id3 = id1; // Copy (Clone not needed for Copy types)

        assert_eq!(id1.as_u16(), 42);
        assert_eq!(id2.as_u16(), 42);
        assert_eq!(id3.as_u16(), 42);
    }

    #[test]
    fn test_user_identifier_equality() {
        let id1 = UserIdentifier::new(42);
        let id2 = UserIdentifier::new(42);
        let id3 = UserIdentifier::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_user_identifier_ordering() {
        let id1 = UserIdentifier::new(10);
        let id2 = UserIdentifier::new(20);

        assert!(id1 < id2);
        assert!(id2 > id1);
        assert!(id1 <= id2);
        assert!(id2 >= id1);
        assert!(id1 <= id1);
        assert!(id1 >= id1);
    }

    #[test]
    fn test_group_identifier_constants() {
        assert_eq!(GroupIdentifier::ROOT.as_u16(), 0);
        assert_eq!(GroupIdentifier::MINIMUM.as_u16(), 1);
        assert_eq!(GroupIdentifier::MAXIMUM.as_u16(), u16::MAX);
    }

    #[test]
    fn test_group_identifier_new() {
        let id = GroupIdentifier::new(42);
        assert_eq!(id.as_u16(), 42);
    }

    #[test]
    fn test_group_identifier_as_u16() {
        let id = GroupIdentifier::new(123);
        assert_eq!(id.as_u16(), 123);
    }

    #[test]
    fn test_group_identifier_add_assign() {
        let mut id = GroupIdentifier::new(10);
        id += 5;
        assert_eq!(id.as_u16(), 15);
    }

    #[test]
    fn test_group_identifier_add() {
        let id1 = GroupIdentifier::new(10);
        let id2 = id1 + 5;
        assert_eq!(id1.as_u16(), 10); // Original unchanged
        assert_eq!(id2.as_u16(), 15);
    }

    #[test]
    fn test_group_identifier_from_u16() {
        let id: GroupIdentifier = 42u16.into();
        assert_eq!(id.as_u16(), 42);
    }

    #[test]
    fn test_group_identifier_to_u16() {
        let id = GroupIdentifier::new(42);
        let value: u16 = id.into();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_group_identifier_clone_copy() {
        let id1 = GroupIdentifier::new(42);
        let id2 = id1; // Copy
        let id3 = id1; // Copy (Clone not needed for Copy types)

        assert_eq!(id1.as_u16(), 42);
        assert_eq!(id2.as_u16(), 42);
        assert_eq!(id3.as_u16(), 42);
    }

    #[test]
    fn test_group_identifier_equality() {
        let id1 = GroupIdentifier::new(42);
        let id2 = GroupIdentifier::new(42);
        let id3 = GroupIdentifier::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_group_identifier_ordering() {
        let id1 = GroupIdentifier::new(10);
        let id2 = GroupIdentifier::new(20);

        assert!(id1 < id2);
        assert!(id2 > id1);
        assert!(id1 <= id2);
        assert!(id2 >= id1);
        assert!(id1 <= id1);
        assert!(id1 >= id1);
    }

    #[test]
    fn test_edge_cases_user_identifier() {
        // Test minimum value
        let min_id = UserIdentifier::new(0);
        assert_eq!(min_id.as_u16(), 0);

        // Test maximum value
        let max_id = UserIdentifier::new(u16::MAX);
        assert_eq!(max_id.as_u16(), u16::MAX);

        // Test overflow behavior (this will wrap around in debug mode)
        let near_max = UserIdentifier::new(u16::MAX - 1);
        let result = near_max + 1;
        assert_eq!(result.as_u16(), u16::MAX);
    }

    #[test]
    fn test_edge_cases_group_identifier() {
        // Test minimum value
        let min_id = GroupIdentifier::new(0);
        assert_eq!(min_id.as_u16(), 0);

        // Test maximum value
        let max_id = GroupIdentifier::new(u16::MAX);
        assert_eq!(max_id.as_u16(), u16::MAX);

        // Test overflow behavior (this will wrap around in debug mode)
        let near_max = GroupIdentifier::new(u16::MAX - 1);
        let result = near_max + 1;
        assert_eq!(result.as_u16(), u16::MAX);
    }

    #[test]
    fn test_hash_consistency() {
        let mut user_map = BTreeMap::new();
        let mut group_map = BTreeMap::new();

        let user_id = UserIdentifier::new(42);
        let group_id = GroupIdentifier::new(42);

        user_map.insert(user_id, "test_user");
        group_map.insert(group_id, "test_group");

        assert_eq!(user_map.get(&user_id), Some(&"test_user"));
        assert_eq!(group_map.get(&group_id), Some(&"test_group"));
    }

    #[test]
    fn test_debug_format() {
        let user_id = UserIdentifier::new(42);
        let group_id = GroupIdentifier::new(42);

        let user_debug = format!("{user_id:?}");
        let group_debug = format!("{group_id:?}");

        assert!(user_debug.contains("42"));
        assert!(group_debug.contains("42"));
    }
}
