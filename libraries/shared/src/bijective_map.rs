use core::fmt::Debug;

use alloc::collections::btree_map::BTreeMap;

pub struct BijectiveBTreeMap<L, R> {
    to_left: BTreeMap<R, L>,
    to_right: BTreeMap<L, R>,
}

impl<L: Ord + Copy, R: Ord + Copy> BijectiveBTreeMap<L, R> {
    pub const fn new() -> Self {
        Self {
            to_left: BTreeMap::new(),
            to_right: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: L, value: R) {
        self.to_left.insert(value, key);
        self.to_right.insert(key, value);
    }

    pub fn get_by_left(&self, key: &L) -> Option<&R> {
        self.to_right.get(key)
    }

    pub fn get_by_right(&self, value: &R) -> Option<&L> {
        self.to_left.get(value)
    }

    pub fn remove_by_key(&mut self, key: &L) -> Option<R> {
        if let Some(value) = self.to_right.remove(key) {
            self.to_left.remove(&value);
            Some(value)
        } else {
            None
        }
    }

    pub fn remove_by_value(&mut self, value: &R) -> Option<L> {
        if let Some(key) = self.to_left.remove(value) {
            self.to_right.remove(&key);
            Some(key)
        } else {
            None
        }
    }

    pub fn get_left_keys(&self) -> impl Iterator<Item = &L> {
        self.to_right.keys()
    }

    pub fn get_right_keys(&self) -> impl Iterator<Item = &R> {
        self.to_left.keys()
    }
}

impl<L: Ord + Copy, R: Ord + Copy> Default for BijectiveBTreeMap<L, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: Ord + Copy + Debug, R: Ord + Copy + Debug> Debug for BijectiveBTreeMap<L, R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BijectiveBTreeMap")
            .field("to_key", &self.to_left)
            .field("to_value", &self.to_right)
            .finish()
    }
}

impl<L: Ord + Copy, R: Ord + Copy> Clone for BijectiveBTreeMap<L, R> {
    fn clone(&self) -> Self {
        Self {
            to_left: self.to_left.clone(),
            to_right: self.to_right.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec, vec::Vec};

    use super::*;

    #[test]
    fn test_new() {
        let map: BijectiveBTreeMap<i32, i32> = BijectiveBTreeMap::new();
        assert!(map.get_by_left(&1).is_none());
        assert!(map.get_by_right(&1).is_none());
    }

    #[test]
    fn test_insert_and_get() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(2, 20);

        assert_eq!(map.get_by_left(&1), Some(&10));
        assert_eq!(map.get_by_left(&2), Some(&20));
        assert_eq!(map.get_by_right(&10), Some(&1));
        assert_eq!(map.get_by_right(&20), Some(&2));
    }

    #[test]
    fn test_insert_overwrite() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(1, 20);

        assert_eq!(map.get_by_left(&1), Some(&20));
        assert!(map.get_by_right(&10).is_some());
    }

    #[test]
    fn test_remove_by_key() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(2, 20);

        assert_eq!(map.remove_by_key(&1), Some(10));
        assert!(map.get_by_left(&1).is_none());
        assert!(map.get_by_right(&10).is_none());
        assert_eq!(map.get_by_left(&2), Some(&20));
    }

    #[test]
    fn test_remove_by_value() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(2, 20);

        assert_eq!(map.remove_by_value(&10), Some(1));
        assert!(map.get_by_left(&1).is_none());
        assert!(map.get_by_right(&10).is_none());
        assert_eq!(map.get_by_left(&2), Some(&20));
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);

        assert_eq!(map.remove_by_key(&2), None);
        assert_eq!(map.remove_by_value(&20), None);
    }

    #[test]
    fn test_iterators() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);

        let left_keys: Vec<_> = map.get_left_keys().copied().collect();
        assert_eq!(left_keys, vec![1, 2, 3]);

        let right_keys: Vec<_> = map.get_right_keys().copied().collect();
        assert_eq!(right_keys, vec![10, 20, 30]);
    }

    #[test]
    fn test_clone() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        map.insert(2, 20);

        let cloned = map.clone();
        assert_eq!(cloned.get_by_left(&1), Some(&10));
        assert_eq!(cloned.get_by_right(&20), Some(&2));
    }

    #[test]
    fn test_default() {
        let map: BijectiveBTreeMap<i32, i32> = BijectiveBTreeMap::default();
        assert!(map.get_by_left(&1).is_none());
    }

    #[test]
    fn test_debug() {
        let mut map = BijectiveBTreeMap::new();
        map.insert(1, 10);
        let debug_str = format!("{:?}", map);
        assert!(debug_str.contains("BijectiveBTreeMap"));
    }
}
