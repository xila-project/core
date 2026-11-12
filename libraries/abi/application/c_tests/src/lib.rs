#[cfg(test)]
mod tests {
    extern crate abi_definitions;
    use abi_application::*;
    use core::ptr;
    testing::instantiate_global_allocator!();

    // C only passes this opaque object through; it never reads the Rust layout.
    #[allow(improper_ctypes)]
    unsafe extern "C" {
        fn application_c_round_trip(directory: *const Directory) -> i32;
    }

    #[task::test]
    async fn real_vfs_and_c_client() {
        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let tasks = Box::leak(Box::new(task::Manager::new()));
        let users = Box::leak(Box::new(users::Manager::new()));
        let task = tasks.get_current_task_identifier().await;
        let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let fs = little_fs::FileSystem::new_format(device, 256).unwrap();
        let vfs = Box::leak(Box::new(
            virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
        ));
        abi_application::initialize(time);
        unsafe {
            let mut mono = 0;
            let mut real = 17;
            assert_eq!(xila_time_monotonic(&mut mono), Status::Ok);
            time.invalidate();
            assert_eq!(xila_time_realtime(&mut real), Status::Unavailable);
            assert_eq!(real, 17);
            time.set_realtime(time::RealtimeSample {
                timestamp: time::Timestamp::from_nanos(i128::from(i64::MAX) + 1_000_000_000),
                resolution: core::time::Duration::from_nanos(1),
                source: time::ClockSource::Manual,
                accuracy: None,
            })
            .unwrap();
            assert_eq!(xila_time_realtime(&mut real), Status::Overflow);
            assert_eq!(real, 17);
        }
        let directory = Directory::new(vfs, task, "/./", 3).unwrap().into_raw();
        unsafe {
            assert_eq!(application_c_round_trip(directory), 0);
            let mut file = ptr::null_mut();
            for (path, length) in [
                (c"/outside".as_ptr(), 8),
                (c"../outside".as_ptr(), 10),
                (c"".as_ptr(), 0),
                (c"nul".as_ptr(), 4),
            ] {
                assert_eq!(
                    xila_file_open(directory, path, length, 1, &mut file),
                    Status::InvalidArgument
                );
                assert!(file.is_null());
            }
            assert_eq!(
                xila_file_open(directory, ptr::null(), 0, 1, &mut file),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_open(directory, c"c-file".as_ptr(), 6, 1, ptr::null_mut()),
                Status::InvalidArgument
            );
            let invalid_utf8 = [255u8];
            assert_eq!(
                xila_file_open(directory, invalid_utf8.as_ptr().cast(), 1, 1, &mut file),
                Status::InvalidArgument
            );

            assert_eq!(
                xila_file_open(directory, c"c-file".as_ptr(), 6, 2 | 4 | 8, &mut file),
                Status::AlreadyExists
            );
            assert!(file.is_null());
            assert_eq!(
                xila_file_open(directory, c"c-file".as_ptr(), 6, 2, &mut file),
                Status::Ok
            );
            let mut count = 99;
            let mut buffer = [0u8; 5];
            assert_eq!(
                xila_file_read(file, buffer.as_mut_ptr().cast(), 5, &mut count),
                Status::PermissionDenied
            );
            assert_eq!(count, 99);
            assert_eq!(
                xila_file_write(file, ptr::null(), 0, &mut count),
                Status::Ok
            );
            assert_eq!(count, 0);
            assert_eq!(
                xila_file_write(file, ptr::null(), 1, &mut count),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_seek(file, -1, 0, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_file_seek(file, 0, 3, ptr::null_mut()),
                Status::InvalidArgument
            );
            assert_eq!(xila_file_close(file), Status::Ok);
            let mut child = ptr::null_mut();
            assert_eq!(
                xila_directory_open(directory, c"c-file".as_ptr(), 6, 1, &mut child),
                Status::NotADirectory
            );
            assert!(child.is_null());
            assert_eq!(
                xila_directory_open(directory, c".".as_ptr(), 1, 1, &mut child),
                Status::Ok
            );
            let mut escalated = ptr::null_mut();
            assert_eq!(
                xila_directory_open(child, c".".as_ptr(), 1, 3, &mut escalated),
                Status::PermissionDenied
            );
            assert_eq!(
                xila_file_open(child, c"c-file".as_ptr(), 6, 2, &mut file),
                Status::PermissionDenied
            );
            assert_eq!(
                xila_file_open(child, c"c-file".as_ptr(), 6, 1, &mut file),
                Status::Ok
            );
            assert_eq!(
                xila_file_read(file, buffer.as_mut_ptr().cast(), 5, &mut count),
                Status::Ok
            );
            assert_eq!(&buffer, b"hello");
            assert_eq!(xila_file_seek(file, -1, 2, ptr::null_mut()), Status::Ok);
            assert_eq!(xila_file_close(file), Status::Ok);
            xila_directory_close(child);
            xila_directory_close(directory);
        }
    }
}
