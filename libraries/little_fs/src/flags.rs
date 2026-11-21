use crate::constants;
use file_system::{AccessFlags, CreateFlags, Flags, StateFlags};

pub fn convert_flags(flags: Flags) -> i32 {
    let mode = flags.get_access();

    let mut little_fs_flags = 0;

    if mode.contains(AccessFlags::Write) {
        little_fs_flags |= constants::OPEN_READ_WRITE;
    } else {
        little_fs_flags |= constants::OPEN_READ_ONLY;
    }

    let open = flags.get_create();

    if open.contains(CreateFlags::Create) {
        little_fs_flags |= constants::OPEN_CREATE;
    }

    if open.contains(CreateFlags::Exclusive) {
        little_fs_flags |= constants::OPEN_EXCLUSIVE;
    }

    if open.contains(CreateFlags::Truncate) {
        little_fs_flags |= constants::OPEN_TRUNCATE;
    }

    if flags.get_state().contains(StateFlags::Append) {
        little_fs_flags |= constants::OPEN_APPEND;
    }

    little_fs_flags
}
