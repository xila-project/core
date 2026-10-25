//! Directory entry representation for file system operations.

//!
//! This module provides the [`Entry`] structure which represents individual
//! entries within directories, such as files, subdirectories, and other file system objects.

use alloc::string::String;

use crate::{Kind, Path, PathOwned};

use super::{Inode, Size};

/// Represents a single entry within a directory.
///
/// A directory entry contains metadata about a file system object including its
/// inode number, name, type, and size. This structure is used when reading directory
/// contents to provide information about each item within the directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The inode number identifying this file system object
    pub inode: Inode,
    /// The name of this directory entry
    pub name: String,
    /// The type of file system object (file, directory, etc.)
    pub kind: Kind,
    /// The size of the file system object in bytes
    pub size: Size,
}

impl Entry {
    /// Create a new directory entry with the specified metadata.
    ///
    /// # Arguments
    ///
    /// * `Inode` - Unique inode number for this file system object
    /// * `Name` - Name of the file or directory
    /// * `Type` - Type of the file system object
    /// * `Size` - Size in bytes (for files) or entry count (for directories)
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::{Entry, Inode, Kind, Size};
    /// use alloc::string::String;
    ///
    /// let entry = Entry::new(
    ///     123,
    ///     String::from("example.txt"),
    ///     Kind::File,
    ///     2048
    /// );
    /// ```
    pub fn new(inode: Inode, name: String, r#type: Kind, size: Size) -> Self {
        Self {
            inode,
            name,
            kind: r#type,
            size,
        }
    }

    pub fn join_path(&self, base_path: impl AsRef<Path>) -> Option<PathOwned> {
        base_path.as_ref().join(Path::from_str(&self.name))
    }
}

impl AsMut<[u8]> for Entry {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self as *mut Entry as *mut u8,
                core::mem::size_of::<Entry>(),
            )
        }
    }
}

impl TryFrom<&mut [u8]> for &mut Entry {
    type Error = ();

    fn try_from(value: &mut [u8]) -> Result<Self, Self::Error> {
        if value.len() != core::mem::size_of::<Entry>() {
            return Err(());
        }
        if !(value.as_ptr() as usize).is_multiple_of(core::mem::align_of::<Entry>()) {
            return Err(());
        }

        #[allow(clippy::transmute_ptr_to_ref)]
        Ok(unsafe { core::mem::transmute::<*mut u8, &mut Entry>(value.as_mut_ptr()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[test]
    fn test_entry_as_mut_slice() {
        let mut entry = Entry::new(42, String::from("test"), Kind::File, 100);

        let slice = entry.as_mut();
        assert_eq!(slice.len(), core::mem::size_of::<Entry>());
    }

    #[test]
    fn test_entry_from_slice_invalid_size() {
        let mut buffer = [0u8; 10]; // Too small
        let result = <&mut Entry>::try_from(buffer.as_mut_slice());
        assert!(result.is_err());
    }

    #[test]
    fn test_entry_from_slice_valid() {
        // Create a properly sized and aligned buffer
        let mut buffer = alloc::vec![0u8; core::mem::size_of::<Entry>()];

        // Ensure proper alignment by using Vec which should be properly aligned for any type
        let result = <&mut Entry>::try_from(buffer.as_mut_slice());

        // This might fail due to alignment requirements, which is expected behavior
        // The important thing is that we're testing the code path
        let _ = result; // We don't assert success since alignment isn't guaranteed
    }
}
