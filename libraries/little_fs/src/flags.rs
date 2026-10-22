use super::littlefs;
use file_system::{AccessFlags, CreateFlags, Flags, StateFlags};

pub fn convert_flags(flags: Flags) -> i32 {
    let mode = flags.get_access();

    let mut little_fs_flags = 0;

    if mode.contains(AccessFlags::Write) {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_RDWR;
    } else {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_RDONLY;
    }

    let open = flags.get_create();

    if open.contains(CreateFlags::Create) {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_CREAT;
    }

    if open.contains(CreateFlags::Exclusive) {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_EXCL;
    }

    if open.contains(CreateFlags::Truncate) {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_TRUNC;
    }

    if flags.get_state().contains(StateFlags::Append) {
        little_fs_flags |= littlefs::lfs_open_flags_LFS_O_APPEND;
    }

    little_fs_flags as i32
}
