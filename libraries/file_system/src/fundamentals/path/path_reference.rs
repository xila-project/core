use core::borrow::Borrow;

use alloc::{borrow::ToOwned, format, string::ToString};

use super::*;

/// A borrowed path type.
/// The implementation is very similar to the standard library's `std::path::Path`.
/// However, this implementation is more lightweight and allows for std-less usage.
#[derive(Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Path(str);

impl Path {
    pub const ROOT: &'static Path = Self::from_str("/");
    pub const EMPTY: &'static Path = Self::from_str("");

    /// Contains the OS core, including the kernel, init system, and critical drivers.
    /// Prevents modification by regular users.
    pub const SYSTEM: &'static Path = Self::from_str("/system");

    /// Stores system-wide settings in a structured format (e.g., JSON, TOML).
    pub const DEVICES: &'static Path = Self::from_str("/devices");
    pub const NETWORK_DEVICES: &'static Path = Self::from_str("/devices/network");

    /// Hardware devices, symlinks for human-friendly names.
    pub const CONFIGURATION: &'static Path = Self::from_str("/configuration");

    /// Contains the shared configurations between applications.
    pub const SHARED_CONFIGURATION: &'static Path = Self::from_str("/configuration/shared");

    /// Binaries data.
    pub const DATA: &'static Path = Self::from_str("/data");

    /// Shared data between binaries.
    pub const SHARED_DATA: &'static Path = Self::from_str("/data/shared");

    /// Contains the system's binaries, including the shell and other executables.
    pub const BINARIES: &'static Path = Self::from_str("/binaries");

    /// Contains the user's data, including documents, downloads, and other files.
    pub const USERS: &'static Path = Self::from_str("/users");

    /// Contains temporary files, including logs and caches.
    pub const TEMPORARY: &'static Path = Self::from_str("/temporary");

    /// Contains logs, including system logs and application logs.
    pub const LOGS: &'static Path = Self::from_str("/temporary/logs");

    /// # Safety
    /// The caller must ensure that the string is a valid path string.
    pub const fn from_str(path: &str) -> &Path {
        unsafe { &*(path as *const str as *const Path) }
    }

    /// # Safety
    /// The caller must ensure that the string is a valid path string.
    pub fn new<S: AsRef<str> + ?Sized>(path: &S) -> &Path {
        unsafe { &*(path.as_ref() as *const str as *const Path) }
    }

    pub fn is_valid(&self) -> bool {
        is_valid_string(&self.0)
    }

    pub fn is_absolute(&self) -> bool {
        self.0.starts_with('/')
    }

    pub fn is_root(&self) -> bool {
        &self.0 == "/"
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_canonical(&self) -> bool {
        self.0.chars().all(|c| c != '.')
    }

    pub fn go_parent(&self) -> Option<&Path> {
        let characters_to_remove = match self.0.rfind(SEPARATOR) {
            Some(index) => index,
            None => {
                // If there is no separator, the path is either empty or relative to the current directory.
                if self.get_length() > 0 {
                    // Relative to the current directory.
                    return Some(Self::EMPTY);
                } else {
                    return None;
                }
            }
        };

        if characters_to_remove == 0 {
            if self.get_length() == 1 {
                return None;
            }

            if self.is_absolute() {
                return Some(Self::ROOT);
            } else {
                return Some(Self::from_str(""));
            }
        }

        Some(Self::from_str(&self.0[..characters_to_remove]))
    }

    pub fn get_file_prefix(&self) -> Option<&str> {
        let extension_start = self
            .0
            .rfind(EXTENSION_SEPARATOR)
            .or_else(|| Some(self.get_length()))?; // Find the extension separator.
        let file_prefix_start = self.0.rfind(SEPARATOR).map(|i| i + 1).unwrap_or(0); // Find the file prefix start.

        if extension_start <= file_prefix_start {
            return None;
        }

        Some(&self.0[file_prefix_start..extension_start])
    }

    pub fn get_file_name(&self) -> Option<&str> {
        let file_prefix_start = self.0.rfind(SEPARATOR).map(|i| i + 1).unwrap_or(0); // Find the file prefix start.

        if file_prefix_start >= self.get_length() {
            return None;
        }

        Some(&self.0[file_prefix_start..])
    }

    pub fn get_extension(&self) -> Option<&str> {
        let extension_start = self.0.rfind(EXTENSION_SEPARATOR)?;

        Some(&self.0[extension_start + 1..])
    }

    pub fn set_extension(&self, extension: &str) -> Option<PathOwned> {
        let extension_start = self
            .0
            .rfind(EXTENSION_SEPARATOR)
            .unwrap_or(self.get_length());

        Some(unsafe {
            PathOwned::new_unchecked(format!("{}{}", &self.0[..extension_start], extension))
        })
    }

    pub fn strip_prefix<'b>(&'b self, path_prefix: &Path) -> Option<&'b Path> {
        let mut stripped_prefix = self.0.strip_prefix(&path_prefix.0)?;

        if stripped_prefix.starts_with(SEPARATOR) {
            stripped_prefix = &stripped_prefix[1..]
        }

        Some(Self::from_str(stripped_prefix))
    }

    pub fn strip_prefix_absolute<'b>(&'b self, path_prefix: &Path) -> Option<&'b Path> {
        if path_prefix.is_root() {
            return Some(self);
        }

        let stripped_prefix = self.0.strip_prefix(&path_prefix.0)?;

        if stripped_prefix.is_empty() {
            return Some(Self::ROOT);
        }

        Some(Self::from_str(stripped_prefix))
    }

    pub fn strip_suffix<'b>(&'b self, path_suffix: &Path) -> Option<&'b Path> {
        let stripped_suffix = self.0.strip_suffix(&path_suffix.0)?;

        if stripped_suffix.ends_with(SEPARATOR) {
            return Some(Self::from_str(
                &stripped_suffix[..stripped_suffix.len() - 1],
            ));
        }

        Some(Self::from_str(stripped_suffix))
    }

    pub fn get_components(&'_ self) -> Components<'_> {
        Components::new(self)
    }

    pub fn join(&self, path: impl AsRef<Path>) -> Option<PathOwned> {
        self.to_owned().join(path)
    }

    pub fn append(&self, path: &str) -> Option<PathOwned> {
        self.to_owned().append(path)
    }

    pub fn get_length(&self) -> usize {
        self.0.len()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(feature = "std")]
impl AsRef<std::path::Path> for Path {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

impl ToOwned for Path {
    type Owned = PathOwned;

    fn to_owned(&self) -> Self::Owned {
        unsafe { PathOwned::new_unchecked(self.0.to_string()) }
    }

    fn clone_into(&self, target: &mut Self::Owned) {
        *target = self.to_owned();
    }
}

impl Borrow<Path> for PathOwned {
    fn borrow(&self) -> &Path {
        Path::from_str(&self.0)
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::from_str(self)
    }
}

impl AsRef<str> for &Path {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_prefix() {
        let path = Path::from_str("/home/user/file.txt");
        let prefix = Path::from_str("/home/user");
        assert_eq!(path.strip_prefix(prefix).unwrap().as_str(), "file.txt");

        let path = Path::from_str("/home/user/file.txt");
        let prefix = Path::from_str("/");
        assert_eq!(
            path.strip_prefix(prefix).unwrap().as_str(),
            "home/user/file.txt"
        );

        let invalid_prefix = Path::from_str("/home/invalid/");
        assert_eq!(path.strip_prefix(invalid_prefix), None);
    }

    #[test]
    fn test_strip_prefix_absolute() {
        let path = Path::from_str("/");
        let prefix = Path::from_str("/");
        assert_eq!(path.strip_prefix_absolute(prefix).unwrap().as_str(), "/");

        let path = Path::from_str("/Foo/Bar");
        let prefix = Path::from_str("/Foo/Bar");
        assert_eq!(path.strip_prefix_absolute(prefix).unwrap().as_str(), "/");

        let path = Path::from_str("/home/user/file.txt");
        let prefix = Path::from_str("/home/user");
        assert_eq!(
            path.strip_prefix_absolute(prefix).unwrap().as_str(),
            "/file.txt"
        );

        let path = Path::from_str("/home/user/file.txt");
        let prefix = Path::from_str("/");
        assert_eq!(
            path.strip_prefix_absolute(prefix).unwrap().as_str(),
            "/home/user/file.txt"
        );

        let invalid_prefix = Path::from_str("/home/invalid/");
        assert_eq!(path.strip_prefix_absolute(invalid_prefix), None);
    }

    #[test]
    fn test_strip_suffix() {
        let path = Path::from_str("/home/user/file.txt");
        let suffix = Path::from_str("user/file.txt");
        assert_eq!(path.strip_suffix(suffix).unwrap().as_str(), "/home");

        let invalid_suffix = Path::from_str("user/invalid.txt");
        assert_eq!(path.strip_suffix(invalid_suffix), None);
    }

    #[test]
    fn test_go_parent() {
        let path = Path::from_str("/home/user/file.txt");
        assert_eq!(&path.go_parent().unwrap().0, "/home/user");
        assert_eq!(&path.go_parent().unwrap().go_parent().unwrap().0, "/home");
        assert_eq!(
            &path
                .go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .0,
            "/"
        );
        assert_eq!(
            path.go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .go_parent(),
            None
        );

        let path = Path::from_str("home/user/file.txt");
        assert_eq!(&path.go_parent().unwrap().0, "home/user");
        assert_eq!(&path.go_parent().unwrap().go_parent().unwrap().0, "home");
        assert_eq!(
            &path
                .go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .go_parent()
                .unwrap()
                .0,
            ""
        );
    }

    #[test]
    fn test_path_file() {
        // Regular case
        let path = Path::from_str("/Directory/File.txt");
        assert_eq!(path.get_extension(), Some("txt"));
        assert_eq!(path.get_file_prefix(), Some("File"));
        assert_eq!(path.get_file_name(), Some("File.txt"));

        // No extension
        let path = Path::from_str("/Directory/File");
        assert_eq!(path.get_extension(), None);
        assert_eq!(path.get_file_prefix(), Some("File"));
        assert_eq!(path.get_file_name(), Some("File"));

        // No file prefix
        let path = Path::from_str("File.txt");
        assert_eq!(path.get_extension(), Some("txt"));
        assert_eq!(path.get_file_prefix(), Some("File"));
        assert_eq!(path.get_file_name(), Some("File.txt"));

        // No file prefix or extension
        let path = Path::from_str("/");
        assert_eq!(path.get_extension(), None);
        assert_eq!(path.get_file_prefix(), None);
        assert_eq!(path.get_file_name(), None);

        // No file prefix or extension
        let path = Path::from_str("File");
        assert_eq!(path.get_extension(), None);
        assert_eq!(path.get_file_prefix(), Some("File"));
        assert_eq!(path.get_file_name(), Some("File"));
    }
}
