use core::{
    fmt::{Display, Formatter},
    ops::Deref,
};

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use super::{EXTENSION_SEPARATOR, Path, SEPARATOR};

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
#[repr(transparent)]
pub struct PathOwned(String);

impl PathOwned {
    /// # Safety
    /// The caller must ensure that the string is valid path string.
    pub unsafe fn new_unchecked(path: String) -> Self {
        PathOwned(path)
    }

    pub fn new(path: String) -> Option<Self> {
        let path = if path.ends_with(SEPARATOR) && path.len() > 1 {
            path[..path.len() - 1].to_string()
        } else {
            path
        };

        if is_valid_string(&path) {
            Some(PathOwned(path))
        } else {
            None
        }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        PathOwned(String::with_capacity(capacity))
    }

    pub fn root() -> PathOwned {
        PathOwned("/".to_string())
    }

    pub fn join(mut self, path: impl AsRef<Path>) -> Option<Self> {
        if path.as_ref().is_absolute() {
            return None;
        }

        if path.as_ref().is_empty() {
            return Some(self);
        }

        if !self.0.ends_with(SEPARATOR) {
            self.0.push(SEPARATOR);
        }
        self.0.push_str(path.as_ref().as_str());

        Some(self)
    }

    pub fn truncate(&mut self, length: usize) {
        self.0.truncate(length);
    }

    pub fn append(self, path: &str) -> Option<Self> {
        self.join(Path::from_str(path))
    }

    pub fn revert_parent_directory(&mut self) -> &mut Self {
        let mut last_index = 0;
        for (i, c) in self.0.chars().enumerate() {
            if c == SEPARATOR {
                last_index = i;
            }
        }
        if last_index == 0 {
            self.0.clear();
            return self;
        }

        self.0.truncate(last_index);
        self
    }

    pub fn get_extension(&self) -> Option<&str> {
        let mut extension = None;

        for (i, c) in self.0.char_indices() {
            if c == EXTENSION_SEPARATOR {
                extension = Some(&self.0[i..]);
            }
        }
        extension
    }

    pub fn get_file_name(&self) -> &str {
        let mut last_index = 0;
        for (i, c) in self.0.chars().enumerate() {
            if c == SEPARATOR {
                last_index = i;
            }
        }
        if last_index >= self.0.len() {
            return &self.0[last_index..];
        }
        &self.0[last_index + 1..]
    }

    pub fn get_relative_to(&self, path: &PathOwned) -> Option<PathOwned> {
        if !self.0.starts_with(path.0.as_str()) {
            return None;
        }

        Some(PathOwned(self.0[path.0.len()..].to_string()))
    }

    pub fn canonicalize(mut self) -> Self {
        let mut stack: Vec<&str> = Vec::new();

        if self.is_absolute() {
            stack.push("");
        }

        for component in self.0.split(SEPARATOR) {
            match component {
                ".." => {
                    stack.pop();
                }
                "." | "" => continue,
                _ => stack.push(component),
            }
        }

        self.0 = stack.join("/");

        if self.0.is_empty() {
            self.0.push('/');
        }

        self
    }
}

pub fn is_valid_string(string: &str) -> bool {
    let invalid = ['\0', ':', '*', '?', '"', '<', '>', '|', ' '];

    for character in string.chars() {
        // Check if the string contains invalid characters.
        if invalid.contains(&character) {
            return false;
        }
    }

    if string.ends_with(SEPARATOR) && string.len() > 1 {
        // Check if the string ends with a separator and is not the root directory.
        return false;
    }

    true
}

impl TryFrom<&str> for PathOwned {
    type Error = ();

    fn try_from(item: &str) -> Result<Self, Self::Error> {
        if is_valid_string(item) {
            Ok(PathOwned(item.to_string()))
        } else {
            Err(())
        }
    }
}

impl TryFrom<String> for PathOwned {
    type Error = ();

    fn try_from(item: String) -> Result<Self, Self::Error> {
        if is_valid_string(&item) {
            Ok(PathOwned(item))
        } else {
            Err(())
        }
    }
}

impl Display for PathOwned {
    fn fmt(&self, formatter: &mut Formatter) -> Result<(), core::fmt::Error> {
        write!(formatter, "{}", self.0)
    }
}

impl AsRef<str> for PathOwned {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Deref for PathOwned {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        Path::from_str(self.0.as_str())
    }
}

impl AsRef<Path> for PathOwned {
    fn as_ref(&self) -> &Path {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_addition() {
        let path = PathOwned::try_from("/").unwrap();
        assert_eq!(path.as_str(), "/");
        let path = path.append("Folder").unwrap();
        assert_eq!(path.as_str(), "/Folder");
        let path = path.append("File").unwrap();
        assert_eq!(path.as_str(), "/Folder/File");
    }

    #[test]
    fn test_valid_string() {
        assert!(is_valid_string("Hello"));
        assert!(is_valid_string("Hello/World"));
        assert!(is_valid_string("Hello/World.txt"));
        assert!(!is_valid_string("Hello/World.txt/"));
        assert!(!is_valid_string("Hello/World.txt:"));
        assert!(!is_valid_string("Hello/World.txt*"));
        assert!(!is_valid_string("Hello/World.txt?"));
        assert!(!is_valid_string("Hello/World.txt\""));
        assert!(!is_valid_string("Hello/World.txt<"));
        assert!(!is_valid_string("Hello/World.txt>"));
        assert!(!is_valid_string("Hello/World.txt|"));
        assert!(!is_valid_string("Hello/World.txt "));
        assert!(!is_valid_string("Hello/World.txt\0"));
        assert!(is_valid_string(""));
        assert!(!is_valid_string("Hello/Wo rld.txt/"));
    }

    #[test]
    fn test_canonicalize() {
        let path = PathOwned::try_from("/home/../home/user/./file.txt").unwrap();
        assert_eq!(path.canonicalize().as_str(), "/home/user/file.txt");

        let path = PathOwned::try_from("./home/../home/user/./file.txt").unwrap();
        assert_eq!(path.canonicalize().as_str(), "home/user/file.txt");
    }
}
