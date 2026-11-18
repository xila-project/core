use core::ptr;

use super::*;

#[test]
fn path_mutations_validate_their_arguments() {
    unsafe {
        assert_eq!(
            xila_file_system_set_times_from_path(0, ptr::null(), 0, 0, 0b0100, false),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_set_times_from_path(0, c"/file".as_ptr(), 0, 0, 0b0011, false),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_set_times_from_path(0, c"/file".as_ptr(), 0, 0, 0, false),
            0
        );
        assert_eq!(
            xila_file_system_link(0, ptr::null(), c"/link".as_ptr()),
            u32::from(Error::InvalidParameter)
        );
        assert_eq!(
            xila_file_system_link(0, c"/file".as_ptr(), ptr::null()),
            u32::from(Error::InvalidParameter)
        );
    }
}
