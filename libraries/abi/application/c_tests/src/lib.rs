#[cfg(test)]
mod tests {
    extern crate abi_definitions;
    use abi_application::*;
    use core::ptr;
    testing::instantiate_global_allocator!();

    #[unsafe(no_mangle)]
    pub extern "C" fn application_c_pause() {
        std::thread::sleep(core::time::Duration::from_millis(1));
    }

    // C only passes this opaque object through; it never reads the Rust layout.
    #[allow(improper_ctypes)]
    unsafe extern "C" {
        fn application_c_round_trip(directory: *const Directory) -> i32;
        fn application_c_file_system(directory: *const Directory) -> i32;
        fn startup_test_main(startup: *const XilaStartup) -> i32;
        fn startup_empty_main(startup: *const XilaStartup) -> i32;
        fn application_c_network(
            network: *const Network,
            denied: *const Network,
            listen_only: *const Network,
        ) -> i32;
    }

    #[task::test]
    async fn real_vfs_and_c_client() {
        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let tasks = Box::leak(Box::new(task::Manager::new()));
        let users = Box::leak(Box::new(users::Manager::new()));
        let task = tasks.get_current_task_identifier().await;
        let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
        let vfs = Box::leak(Box::new(
            virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
        ));
        abi_application::initialize(time);
        virtual_file_system::create_default_hierarchy(vfs, task)
            .await
            .unwrap();
        let network: &'static network::Manager = Box::leak(Box::new(network::Manager::new(
            tasks,
            vfs,
            time,
            &testing::RandomDevice,
        )));
        // The C client blocks its thread, so the interface runner needs its own
        // executor. `mount_interface` spawns the runner on the calling executor:
        // mount from a task sent to that thread.
        let spawner = testing::new_thread_executor(tasks).await;
        tasks
            .spawn_send(task, "Mount loopback", Some(spawner), move |_| async move {
                let (device, controller) = network::create_loopback_device();
                network
                    .mount_interface(task, "loopback0", device, controller, None)
                    .await
            })
            .await
            .unwrap()
            .0
            .join()
            .await
            .unwrap();
        let mut interface = virtual_file_system::File::open(
            vfs,
            task,
            "/devices/network/loopback0",
            file_system::AccessFlags::Write.into(),
        )
        .await
        .unwrap();
        interface
            .control(
                network::ADD_IP_ADDRESS,
                &network::IpCidr::new_ipv4([127, 0, 0, 1], 8),
            )
            .await
            .unwrap();
        interface.close(vfs).await.unwrap();
        assert!(matches!(
            Network::new(network, 8),
            Err(Status::InvalidArgument)
        ));
        let authority = Network::new(network, 7).unwrap();
        let denied = Network::new(network, 0).unwrap();
        let listen_only = Network::new(network, XILA_NETWORK_LISTEN).unwrap();
        unsafe {
            assert_eq!(application_c_network(&authority, &denied, &listen_only), 0);
        }

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
        time.set_realtime(time::RealtimeSample {
            timestamp: time::Timestamp::from_nanos(1_700_000_000_000_000_000),
            resolution: core::time::Duration::from_secs(1),
            source: time::ClockSource::Manual,
            accuracy: None,
        })
        .unwrap();
        unsafe {
            assert_eq!(
                run_main(startup_empty_main, &[], None, None, None, None),
                -7
            );
            let path = file_system::Path::from_str("/startup-output");
            let output = virtual_file_system::SynchronousFile::open(
                vfs,
                task,
                path,
                file_system::Flags::new(
                    file_system::AccessFlags::READ_WRITE,
                    Some(file_system::CreateFlags::Create),
                    None,
                ),
            )
            .unwrap();
            assert_eq!(
                run_main(
                    startup_test_main,
                    &["", "héllo"],
                    None,
                    Some(File::new(output)),
                    None,
                    Some(Directory::new(vfs, task, "/", 3).unwrap())
                ),
                23
            );
            let mut output = virtual_file_system::SynchronousFile::open(
                vfs,
                task,
                path,
                file_system::AccessFlags::Read.into(),
            )
            .unwrap();
            let mut bytes = [0; 7];
            assert_eq!(output.read(&mut bytes).unwrap(), 7);
            assert_eq!(&bytes, b"startup");
            let home_file = virtual_file_system::SynchronousFile::open(
                vfs,
                task,
                file_system::Path::from_str("/startup-home"),
                file_system::AccessFlags::Read.into(),
            )
            .unwrap();
            drop(home_file);
        }
        let directory = Directory::new(vfs, task, "/./", 3).unwrap().into_raw();
        unsafe {
            assert_eq!(application_c_round_trip(directory), 0);
            assert_eq!(application_c_file_system(directory), 0);
            // Validate conversion independently from the C client, including overflow.
            let mut source = vfs
                .get_statistics_for_task(task, &file_system::Path::ROOT)
                .await
                .unwrap();
            source.access = file_system::Time::new(1);
            source.modification = file_system::Time::new(2);
            source.status = file_system::Time::new(3);
            let statistics = Statistics::try_from(source.clone()).unwrap();
            assert_eq!(statistics.access_nanoseconds, 1_000_000_000);
            assert_eq!(statistics.modification_nanoseconds, 2_000_000_000);
            assert_eq!(statistics.status_change_nanoseconds, 3_000_000_000);
            source.access = file_system::Time::new(u64::MAX);
            assert_eq!(Statistics::try_from(source), Err(Status::Overflow));
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

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn stackful_file_read_yields_to_executor_peer() {
        use core::task::{Context as TaskContext, Poll, Waker};
        use file_system::{
            BaseOperations, CharacterDevice, Context as DeviceContext, MountOperations, Operation,
            Size,
        };
        use portable_atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Mutex;

        #[derive(Default)]
        struct ReadState {
            ready: AtomicBool,
            polls: AtomicUsize,
            waker: Mutex<Option<Waker>>,
        }
        struct PendingDevice(&'static ReadState);
        impl BaseOperations for PendingDevice {
            fn read(
                &self,
                _: &mut DeviceContext,
                cx: &mut TaskContext<'_>,
                buffer: &mut [u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                let polls = self.0.polls.fetch_add(1, Ordering::Relaxed);
                if self.0.ready.load(Ordering::Relaxed) {
                    buffer.copy_from_slice(b"ok");
                    return Poll::Ready(Ok(2));
                }
                // Fail promptly if the caller polls again without letting the peer run.
                if polls != 0 {
                    return Poll::Ready(Err(file_system::Error::UnsupportedOperation));
                }
                *self.0.waker.lock().unwrap() = Some(cx.waker().clone());
                Poll::Pending
            }

            fn write(
                &self,
                _: &mut DeviceContext,
                _: &mut TaskContext<'_>,
                _: &[u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                Poll::Ready(Err(file_system::Error::UnsupportedOperation))
            }

            fn clone_context(&self, _: &DeviceContext) -> file_system::Result<DeviceContext> {
                Ok(DeviceContext::new_empty())
            }

            fn cancel_operation(&self, _: &mut DeviceContext, _: Operation) {
                self.0.waker.lock().unwrap().take();
            }
        }
        impl MountOperations for PendingDevice {}
        impl CharacterDevice for PendingDevice {}

        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let tasks = task::test_manager();
        let users = Box::leak(Box::new(users::Manager::new()));
        let task = tasks.get_current_task_identifier().await;
        let spawner = tasks.get_spawner(task).await.unwrap();
        let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
        let vfs = Box::leak(Box::new(
            virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
        ));
        virtual_file_system::create_default_hierarchy(vfs, task)
            .await
            .unwrap();

        let state: &'static ReadState = Box::leak(Box::new(ReadState::default()));
        vfs.mount_character_device(task, "/devices/pending", PendingDevice(state))
            .await
            .unwrap();

        let root = Directory::new(vfs, task, "/", XILA_RIGHT_READ).unwrap();
        let path = c"devices/pending";
        let mut file = ptr::null_mut();
        unsafe {
            assert_eq!(
                xila_file_open(
                    &root,
                    path.as_ptr(),
                    path.count_bytes(),
                    XILA_OPEN_READ,
                    &mut file
                ),
                Status::Ok
            );
            let mut buffer = [0; 2];
            let mut count = 99;
            let status = xila_file_read(file, buffer.as_mut_ptr().cast(), buffer.len(), &mut count);
            assert_eq!(status, Status::WouldBlock);
            assert_eq!(count, 99);
        }
        assert_eq!(state.polls.swap(0, Ordering::Relaxed), 1);
        assert!(state.waker.lock().unwrap().is_none());

        // SAFETY: The bounded ABI call chain fits within the supplied stack.
        // The resulting task owns the handle until xila_file_close.
        let (reader, _) = unsafe {
            stackful::spawn(
                tasks,
                task,
                "ABI reader",
                stackful::Stack::new(64 * 1024),
                move || {
                    let mut buffer = [0; 2];
                    let mut count = 0;
                    let status =
                        xila_file_read(file, buffer.as_mut_ptr().cast(), buffer.len(), &mut count);
                    let close = xila_file_close(file);
                    i32::from(
                        status != Status::Ok
                            || close != Status::Ok
                            || count != 2
                            || &buffer != b"ok",
                    )
                },
            )
            .await
            .unwrap()
        };
        let (peer, _) = tasks
            .spawn(
                task,
                "device readiness",
                Some(spawner),
                move |_| async move {
                    while state.polls.load(Ordering::Relaxed) == 0 {
                        task::yield_now().await;
                    }
                    assert_eq!(state.polls.load(Ordering::Relaxed), 1);
                    state.ready.store(true, Ordering::Relaxed);
                    state.waker.lock().unwrap().take().unwrap().wake();
                },
            )
            .await
            .unwrap();

        assert_eq!(reader.join().await, 0);
        peer.join().await;
        assert_eq!(state.polls.load(Ordering::Relaxed), 2);
    }
}
