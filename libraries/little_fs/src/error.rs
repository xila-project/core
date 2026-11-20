use super::littlefs;
use file_system::{IoErrorKind, Result};

pub(crate) fn convert_result(error: i32) -> Result<u32> {
    match error {
        littlefs::lfs_error_LFS_ERR_IO => Err(IoErrorKind::InputOutput),
        littlefs::lfs_error_LFS_ERR_CORRUPT => Err(IoErrorKind::CorruptData),
        littlefs::lfs_error_LFS_ERR_NOENT => Err(IoErrorKind::NotFound),
        littlefs::lfs_error_LFS_ERR_EXIST => Err(IoErrorKind::AlreadyExists),
        littlefs::lfs_error_LFS_ERR_NOTDIR => Err(IoErrorKind::NotADirectory),
        littlefs::lfs_error_LFS_ERR_ISDIR => Err(IoErrorKind::IsADirectory),
        littlefs::lfs_error_LFS_ERR_NOTEMPTY => Err(IoErrorKind::DirectoryNotEmpty),
        littlefs::lfs_error_LFS_ERR_BADF => Err(IoErrorKind::BadDescriptor),
        littlefs::lfs_error_LFS_ERR_FBIG => Err(IoErrorKind::FileTooLarge),
        littlefs::lfs_error_LFS_ERR_INVAL => Err(IoErrorKind::InvalidArgument),
        littlefs::lfs_error_LFS_ERR_NOSPC => Err(IoErrorKind::NoSpace),
        littlefs::lfs_error_LFS_ERR_NOMEM => Err(IoErrorKind::OutOfMemory),
        littlefs::lfs_error_LFS_ERR_NOATTR => Err(IoErrorKind::MissingAttribute),
        littlefs::lfs_error_LFS_ERR_NAMETOOLONG => Err(IoErrorKind::NameTooLong),
        _ => {
            if error >= littlefs::lfs_error_LFS_ERR_OK {
                Ok(error as u32)
            } else {
                Err(IoErrorKind::InputOutput)
            }
        }
    }
}
