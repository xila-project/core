use core::str::Split;

use super::{Path, SEPARATOR};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component<'a> {
    Root,
    Current,
    Parent,
    Normal(&'a str),
}

impl<'a> From<&'a str> for Component<'a> {
    fn from(item: &'a str) -> Self {
        match item {
            "" => Component::Root,
            "/" => Component::Root,
            "." => Component::Current,
            ".." => Component::Parent,
            _ => Component::Normal(item),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Components<'a>(Split<'a, char>);

impl<'a> Components<'a> {
    pub fn new(path: &'_ Path) -> Components<'_> {
        Components(path.as_str().split(SEPARATOR))
    }

    pub fn strip_prefix(self, components: &Components<'a>) -> Option<Components<'a>> {
        let mut self_iter = self.clone();
        let components_iter = components.clone();

        for component in components_iter {
            match self_iter.next() {
                Some(self_component) if self_component == component => {}
                _ => return None,
            }
        }

        Some(Components(self_iter.0))
    }

    pub fn get_common_components(self, other: Components<'a>) -> usize {
        self.zip(other).take_while(|(a, b)| a == b).count()
    }
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Component::from)
    }
}

impl DoubleEndedIterator for Components<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(Component::from)
    }
}

#[cfg(test)]
mod tests {

    use alloc::{vec, vec::Vec};

    use super::*;

    #[test]
    fn test_components() {
        assert_eq!(
            Components::new(Path::from_str("/a/b/c")).collect::<Vec<_>>(),
            vec![
                Component::Root,
                Component::Normal("a"),
                Component::Normal("b"),
                Component::Normal("c")
            ]
        );

        assert_eq!(
            Components::new(Path::from_str("/a/./b/c")).collect::<Vec<_>>(),
            vec![
                Component::Root,
                Component::Normal("a"),
                Component::Current,
                Component::Normal("b"),
                Component::Normal("c")
            ]
        );

        assert_eq!(
            Components::new(Path::from_str("a/b/c")).collect::<Vec<_>>(),
            vec![
                Component::Normal("a"),
                Component::Normal("b"),
                Component::Normal("c")
            ]
        );

        assert_eq!(
            Components::new(Path::from_str("a/./../b/c")).collect::<Vec<_>>(),
            vec![
                Component::Normal("a"),
                Component::Current,
                Component::Parent,
                Component::Normal("b"),
                Component::Normal("c")
            ]
        );
    }
}
