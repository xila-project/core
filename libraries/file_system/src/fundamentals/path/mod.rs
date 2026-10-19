/// Path module of the File_system library.
///
/// This module contains the different types that represent paths.
///
/// A path is a string that represents the location of a file or directory in a file system.
/// It is composed of components separated by a separator character.
/// A file path should start with a [Separator] and not end with a [Separator].
mod components;
mod path_owned;
mod path_reference;

pub use components::*;
pub use path_owned::*;
pub use path_reference::*;

/// Separator character used in paths.
pub const SEPARATOR: char = '/';

/// Extension separator character used in paths.
pub const EXTENSION_SEPARATOR: char = '.';
