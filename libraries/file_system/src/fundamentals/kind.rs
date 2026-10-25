//! File system object type definitions.
//!
//! This module provides the [`Kind`] enumeration which categorizes different
//! types of objects that can exist in a file system, such as regular files,
//! directories, devices, and special file types.

use core::fmt::Display;

/// Enumeration of file system object types.
///
/// This enum represents the different types of objects that can exist in a file system.
/// Each variant corresponds to a specific type of file system entity with its own
/// characteristics and behaviors.
///
/// # Examples
///
/// ```rust
/// use file_system::Kind;
///
/// let file_type = Kind::File;
/// let dir_type = Kind::Directory;
///
/// // Types can be compared
/// assert_ne!(file_type, dir_type);
///
/// // Types can be displayed
/// println!("Object type: {}", file_type); // Prints "Object type: File"
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Kind {
    /// Regular file containing data
    File = 1,
    /// Directory containing other file system objects
    Directory,
    /// Block-oriented device (e.g., disk drives, storage devices)
    BlockDevice,
    /// Character-oriented device (e.g., terminals, serial ports)
    CharacterDevice,
    /// Named pipe (FIFO) for inter-process communication
    Pipe,
    /// Unix domain socket for inter-process communication
    Socket,
    /// Symbolic link pointing to another file system object
    SymbolicLink,
}

impl Display for Kind {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let type_value = match self {
            Kind::File => "File",
            Kind::Directory => "Directory",
            Kind::BlockDevice => "Block device",
            Kind::CharacterDevice => "Character device",
            Kind::Pipe => "Pipe",
            Kind::Socket => "Socket",
            Kind::SymbolicLink => "Symbolic link",
        };

        write!(f, "{type_value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_type_variants() {
        // Test that all variants can be created
        let file = Kind::File;
        let directory = Kind::Directory;
        let block_device = Kind::BlockDevice;
        let character_device = Kind::CharacterDevice;
        let pipe = Kind::Pipe;
        let socket = Kind::Socket;
        let symbolic_link = Kind::SymbolicLink;

        // Basic verification they're different
        assert_ne!(file, directory);
        assert_ne!(directory, block_device);
        assert_ne!(block_device, character_device);
        assert_ne!(character_device, pipe);
        assert_ne!(pipe, socket);
        assert_ne!(socket, symbolic_link);
    }

    #[test]
    fn test_type_equality() {
        let file1 = Kind::File;
        let file2 = Kind::File;
        let directory = Kind::Directory;

        assert_eq!(file1, file2);
        assert_ne!(file1, directory);
    }

    #[test]
    fn test_type_clone_copy() {
        let original = Kind::File;
        let cloned = original;
        let copied = original;

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
        assert_eq!(cloned, copied);
    }

    #[test]
    fn test_type_debug() {
        let file_type = Kind::File;
        let debug_str = format!("{file_type:?}");
        assert_eq!(debug_str, "File");

        let directory_type = Kind::Directory;
        let debug_str = format!("{directory_type:?}");
        assert_eq!(debug_str, "Directory");
    }

    #[test]
    fn test_type_display() {
        assert_eq!(format!("{}", Kind::File), "File");
        assert_eq!(format!("{}", Kind::Directory), "Directory");
        assert_eq!(format!("{}", Kind::BlockDevice), "Block device");
        assert_eq!(format!("{}", Kind::CharacterDevice), "Character device");
        assert_eq!(format!("{}", Kind::Pipe), "Pipe");
        assert_eq!(format!("{}", Kind::Socket), "Socket");
        assert_eq!(format!("{}", Kind::SymbolicLink), "Symbolic link");
    }

    #[test]
    fn test_type_repr() {
        // Test that the enum has a specific memory representation
        use core::mem::size_of;

        // Should be 1 byte due to repr(u8)
        assert_eq!(size_of::<Kind>(), 1);
    }

    #[test]
    fn test_type_discriminants() {
        // Test that different variants have different discriminants
        let file = Kind::File as u8;
        let directory = Kind::Directory as u8;
        let block_device = Kind::BlockDevice as u8;
        let character_device = Kind::CharacterDevice as u8;
        let pipe = Kind::Pipe as u8;
        let socket = Kind::Socket as u8;
        let symbolic_link = Kind::SymbolicLink as u8;

        // All should be different
        let discriminants = [
            file,
            directory,
            block_device,
            character_device,
            pipe,
            socket,
            symbolic_link,
        ];

        for i in 0..discriminants.len() {
            for j in i + 1..discriminants.len() {
                assert_ne!(
                    discriminants[i], discriminants[j],
                    "Discriminants {i} and {j} should be different"
                );
            }
        }
    }

    #[test]
    fn test_type_pattern_matching() {
        let file_type = Kind::File;

        let description = match file_type {
            Kind::File => "regular file",
            Kind::Directory => "directory",
            Kind::BlockDevice => "block device",
            Kind::CharacterDevice => "character device",
            Kind::Pipe => "pipe",
            Kind::Socket => "socket",
            Kind::SymbolicLink => "symbolic link",
        };

        assert_eq!(description, "regular file");
    }

    #[test]
    fn test_type_all_variants_pattern_matching() {
        let types = [
            Kind::File,
            Kind::Directory,
            Kind::BlockDevice,
            Kind::CharacterDevice,
            Kind::Pipe,
            Kind::Socket,
            Kind::SymbolicLink,
        ];

        for type_variant in types.iter() {
            // Test that each variant can be matched
            let _matched = match type_variant {
                Kind::File => "file",
                Kind::Directory => "directory",
                Kind::BlockDevice => "block",
                Kind::CharacterDevice => "char",
                Kind::Pipe => "pipe",
                Kind::Socket => "socket",
                Kind::SymbolicLink => "symlink",
            };
        }
    }

    #[test]
    fn test_type_in_collections() {
        let types = [Kind::File, Kind::Directory, Kind::SymbolicLink];

        assert_eq!(types.len(), 3);
        assert_eq!(types[0], Kind::File);
        assert_eq!(types[1], Kind::Directory);
        assert_eq!(types[2], Kind::SymbolicLink);
    }

    #[test]
    fn test_type_is_specific_type() {
        // Helper functions that might be useful
        fn is_file(t: Kind) -> bool {
            matches!(t, Kind::File)
        }

        fn is_directory(t: Kind) -> bool {
            matches!(t, Kind::Directory)
        }

        fn is_device(t: Kind) -> bool {
            matches!(t, Kind::BlockDevice | Kind::CharacterDevice)
        }

        assert!(is_file(Kind::File));
        assert!(!is_file(Kind::Directory));

        assert!(is_directory(Kind::Directory));
        assert!(!is_directory(Kind::File));

        assert!(is_device(Kind::BlockDevice));
        assert!(is_device(Kind::CharacterDevice));
        assert!(!is_device(Kind::File));
    }

    #[test]
    fn test_type_default_behavior() {
        // Test that we can use Kind in various contexts
        let mut type_counts = alloc::collections::BTreeMap::new();

        let types = [
            Kind::File,
            Kind::File,
            Kind::Directory,
            Kind::File,
            Kind::SymbolicLink,
        ];

        for t in types.iter() {
            *type_counts.entry(*t).or_insert(0) += 1;
        }

        assert_eq!(type_counts[&Kind::File], 3);
        assert_eq!(type_counts[&Kind::Directory], 1);
        assert_eq!(type_counts[&Kind::SymbolicLink], 1);
    }
}
