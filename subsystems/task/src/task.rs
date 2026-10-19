use core::fmt::Display;

#[cfg(target_pointer_width = "32")]
pub type TaskIdentifierInner = u16;
#[cfg(target_pointer_width = "64")]
pub type TaskIdentifierInner = u32;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct TaskIdentifier(TaskIdentifierInner);

impl TaskIdentifier {
    pub const MAXIMUM: TaskIdentifierInner = TaskIdentifierInner::MAX;
    pub const MINIMUM: TaskIdentifierInner = TaskIdentifierInner::MIN;
}

impl Display for TaskIdentifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TaskIdentifier {
    pub const fn new(identifier: TaskIdentifierInner) -> Self {
        Self(identifier)
    }

    pub const fn into_inner(self) -> TaskIdentifierInner {
        self.0
    }
}

impl From<usize> for TaskIdentifier {
    fn from(value: usize) -> Self {
        Self(value as TaskIdentifierInner)
    }
}

impl From<TaskIdentifier> for usize {
    fn from(value: TaskIdentifier) -> Self {
        value.0 as usize
    }
}

impl From<TaskIdentifierInner> for TaskIdentifier {
    fn from(value: TaskIdentifierInner) -> Self {
        Self(value)
    }
}

impl From<TaskIdentifier> for TaskIdentifierInner {
    fn from(value: TaskIdentifier) -> Self {
        value.0
    }
}

impl PartialEq<TaskIdentifierInner> for TaskIdentifier {
    fn eq(&self, other: &TaskIdentifierInner) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    #[test]
    fn test_task_identifier_constants() {
        assert_eq!(TaskIdentifier::MAXIMUM, TaskIdentifierInner::MAX);
        assert_eq!(TaskIdentifier::MINIMUM, TaskIdentifierInner::MIN);
    }

    #[test]
    fn test_task_identifier_new() {
        let id = TaskIdentifier::new(42);
        assert_eq!(id.into_inner(), 42);
    }

    #[test]
    fn test_task_identifier_into_inner() {
        let inner_value = 123;
        let id = TaskIdentifier::new(inner_value);
        assert_eq!(id.into_inner(), inner_value);
    }

    #[test]
    fn test_task_identifier_from_inner_type() {
        let inner_value = 456;
        let id: TaskIdentifier = inner_value.into();
        assert_eq!(id.into_inner(), inner_value);
    }

    #[test]
    fn test_task_identifier_into_inner_type() {
        let id = TaskIdentifier::new(789);
        let inner_value: TaskIdentifierInner = id.into();
        assert_eq!(inner_value, 789);
    }

    #[test]
    fn test_task_identifier_clone_copy() {
        let id1 = TaskIdentifier::new(42);
        let id2 = id1; // Copy
        let id3 = id1; // Copy (Clone not needed for Copy types)

        assert_eq!(id1.into_inner(), 42);
        assert_eq!(id2.into_inner(), 42);
        assert_eq!(id3.into_inner(), 42);
    }

    #[test]
    fn test_task_identifier_equality() {
        let id1 = TaskIdentifier::new(42);
        let id2 = TaskIdentifier::new(42);
        let id3 = TaskIdentifier::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_task_identifier_equality_with_inner_type() {
        let id = TaskIdentifier::new(42);
        let inner_value = 42;
        let different_value = 43;

        assert_eq!(id, inner_value);
        assert_ne!(id, different_value);
    }

    #[test]
    fn test_task_identifier_ordering() {
        let id1 = TaskIdentifier::new(10);
        let id2 = TaskIdentifier::new(20);

        assert!(id1 < id2);
        assert!(id2 > id1);
        assert!(id1 <= id2);
        assert!(id2 >= id1);
        assert!(id1 <= id1);
        assert!(id1 >= id1);
    }

    #[test]
    fn test_task_identifier_hash_consistency() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        let id = TaskIdentifier::new(42);

        map.insert(id, "test_task");
        assert_eq!(map.get(&id), Some(&"test_task"));
    }

    #[test]
    fn test_task_identifier_debug_format() {
        let id = TaskIdentifier::new(42);
        let debug_str = format!("{id:?}");
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_edge_cases() {
        // Test minimum value
        let min_id = TaskIdentifier::new(TaskIdentifier::MINIMUM);
        assert_eq!(min_id.into_inner(), TaskIdentifier::MINIMUM);

        // Test maximum value
        let max_id = TaskIdentifier::new(TaskIdentifier::MAXIMUM);
        assert_eq!(max_id.into_inner(), TaskIdentifier::MAXIMUM);
    }

    #[test]
    fn test_const_constructor() {
        // Test that the constructor can be used in const context
        const ID: TaskIdentifier = TaskIdentifier::new(100);
        assert_eq!(ID.into_inner(), 100);
    }

    #[test]
    fn test_const_into_inner() {
        // Test that into_inner can be used in const context
        const ID: TaskIdentifier = TaskIdentifier::new(200);
        const INNER: TaskIdentifierInner = ID.into_inner();
        assert_eq!(INNER, 200);
    }

    #[test]
    fn test_repr_transparent() {
        // Test that the size is the same as the inner type (due to #[repr(transparent)])
        assert_eq!(
            std::mem::size_of::<TaskIdentifier>(),
            std::mem::size_of::<TaskIdentifierInner>()
        );
    }

    #[test]
    fn test_different_architectures() {
        // This test verifies the conditional compilation works correctly
        #[cfg(target_pointer_width = "32")]
        {
            assert_eq!(std::mem::size_of::<Task_identifier_inner_type>(), 2); // u16
        }

        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(std::mem::size_of::<TaskIdentifierInner>(), 4); // u32
        }
    }

    #[test]
    fn test_zero_value() {
        let zero_id = TaskIdentifier::new(0);
        assert_eq!(zero_id.into_inner(), 0);
        assert_eq!(zero_id, 0);
    }

    #[test]
    fn test_bidirectional_conversion() {
        let original_value = 12345;
        let id = TaskIdentifier::new(original_value);
        let converted_back: TaskIdentifierInner = id.into();
        let id_again: TaskIdentifier = converted_back.into();

        assert_eq!(original_value, converted_back);
        assert_eq!(id, id_again);
    }
}
