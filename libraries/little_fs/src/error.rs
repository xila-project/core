use super::littlefs;
use file_system::{Error, Result};

/*
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Error {
    // LittleFS errors
    Input_output,        // Error during device operation
    Corrupted,           // Corrupted
    No_Entry,            // No directory entry
    Entry_exists,        // Entry already exists
    Not_directory,       // Entry is not a dir
    is_directory,        // Entry is a dir
    Directory_not_empty, // Dir is not empty
    Bad_file_number,     // Bad file number
    File_too_large,      // File too large
    InvalidParameter,   // Invalid parameter
    No_space_left,       // No space left on device
    No_memory,           // No more memory available
    No_attribute,        // No data/attr available
    Name_too_long,       // File name too long
    // Custom errors
    Too_many_open_files, // No file identifier left
    Internal_error,      // Internal error
    Poisoned_lock,       // Poisoned lock
    Invalid_identifier,  // Invalid file identifier
}
    */

pub(crate) fn convert_result(error: i32) -> Result<u32> {
    match error {
        littlefs::lfs_error_LFS_ERR_IO => Err(Error::InputOutput),
        littlefs::lfs_error_LFS_ERR_CORRUPT => Err(Error::Corrupted),
        littlefs::lfs_error_LFS_ERR_NOENT => Err(Error::NotFound),
        littlefs::lfs_error_LFS_ERR_EXIST => Err(Error::AlreadyExists),
        littlefs::lfs_error_LFS_ERR_NOTDIR => Err(Error::NotDirectory),
        littlefs::lfs_error_LFS_ERR_ISDIR => Err(Error::IsDirectory),
        littlefs::lfs_error_LFS_ERR_NOTEMPTY => Err(Error::DirectoryNotEmpty),
        littlefs::lfs_error_LFS_ERR_BADF => Err(Error::InvalidIdentifier),
        littlefs::lfs_error_LFS_ERR_FBIG => Err(Error::FileTooLarge),
        littlefs::lfs_error_LFS_ERR_INVAL => Err(Error::InvalidParameter),
        littlefs::lfs_error_LFS_ERR_NOSPC => Err(Error::NoSpaceLeft),
        littlefs::lfs_error_LFS_ERR_NOMEM => Err(Error::NoMemory),
        littlefs::lfs_error_LFS_ERR_NOATTR => Err(Error::NoAttribute),
        littlefs::lfs_error_LFS_ERR_NAMETOOLONG => Err(Error::NameTooLong),
        _ => {
            if error >= littlefs::lfs_error_LFS_ERR_OK {
                Ok(error as u32)
            } else {
                Err(Error::InternalError)
            }
        }
    }
}
/*
impl From<Error> for crate::Error {
    fn from(Error: Error) -> Self {
        match Error {
            Error::Input_output => Self::Input_output,
            Error::Corrupted => Self::Corrupted,
            Error::No_Entry => Self::Not_found,
            Error::Entry_exists => Self::Already_exists,
            Error::Not_directory => Self::Not_directory,
            Error::is_directory => Self::is_directory,
            Error::Directory_not_empty => Self::Directory_not_empty,
            Error::Bad_file_number => Self::Invalid_identifier,
            Error::File_too_large => Self::File_too_large,
            Error::InvalidParameter => Self::Invalid_input,
            Error::No_space_left => Self::File_system_full,
            Error::No_memory => Self::No_memory,
            Error::No_attribute => Self::No_attribute,
            Error::Name_too_long => Self::Name_too_long,
            Error::Too_many_open_files => Self::Too_many_open_files,
            Error::Internal_error => Self::Internal_error,
            Error::Poisoned_lock => Self::Internal_error,
            Error::Invalid_identifier => Self::Invalid_identifier,
        }
    }
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_: PoisonError<T>) -> Self {
        Error::Poisoned_lock
    }
}*/
