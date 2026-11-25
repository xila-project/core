#[cfg(test)]
mod tests {
    use abi_application::*;
    use core::ptr::{self, NonNull};

    /// The heap and host state of a test run. They are leaked, like the other
    /// fixtures here, so the coroutine entry holds only pointers to them.
    fn run_state(
        arguments: Vec<String>,
        input: Option<File>,
        output: Option<File>,
        error: Option<File>,
    ) -> (NonNull<HostState>, NonNull<Heap>) {
        (
            NonNull::from(Box::leak(Box::new(HostState::new(
                arguments, input, output, error,
            )))),
            NonNull::from(Box::leak(Box::new(Heap::new()))),
        )
    }
    testing::instantiate_global_allocator!();

    #[unsafe(no_mangle)]
    pub extern "C" fn application_c_pause() {
        std::thread::sleep(core::time::Duration::from_millis(1));
    }

    // C only passes this opaque object through; it never reads the Rust layout.
    #[allow(improper_ctypes)]
    unsafe extern "C" {
        fn application_c_round_trip(api: *const XilaApi, directory: *const Directory) -> i32;
        fn application_c_file_system(api: *const XilaApi, directory: *const Directory) -> i32;
        fn startup_test_main(startup: *const XilaStartup) -> i32;
        fn sdk_test_versions();
        fn sdk_test_startup(startup: *const XilaStartup) -> i32;
        fn startup_heap_main(startup: *const XilaStartup) -> i32;
        fn startup_empty_main(startup: *const XilaStartup) -> i32;
        fn startup_clock_random_main(startup: *const XilaStartup) -> i32;
        fn threads_main(startup: *const XilaStartup) -> i32;
        fn process_parent_main(startup: *const XilaStartup) -> i32;
        fn process_test_entry(name: *const core::ffi::c_char, length: usize) -> Option<ChildEntry>;
        fn application_c_network(
            api: *const XilaApi,
            network: *const Network,
            denied: *const Network,
            listen_only: *const Network,
            cooperative: bool,
        ) -> i32;
    }

    #[test]
    fn sdk_checks_compatibility_at_major_and_minor_boundaries() {
        unsafe { sdk_test_versions() };
    }

    type ChildEntry = unsafe extern "C" fn(*const XilaStartup) -> i32;

    /// Starts the C children of `process.c` as coroutine tasks of the caller's executor,
    /// choosing the entry by the file name of the spawned path, as Core's host does
    /// with the executable it finds there.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    struct TestHost {
        tasks: &'static task::Manager,
        vfs: &'static virtual_file_system::VirtualFileSystem,
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    impl ProcessHost for TestHost {
        fn task_manager(&self) -> &'static task::Manager {
            self.tasks
        }
        fn virtual_file_system(&self) -> &'static virtual_file_system::VirtualFileSystem {
            self.vfs
        }
        fn spawn(&'static self, request: SpawnRequest) -> SpawnFuture {
            Box::pin(async move {
                let name = request
                    .path
                    .as_str()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string();
                // SAFETY: the C table only reads the name.
                let entry = unsafe { process_test_entry(name.as_ptr().cast(), name.len()) }
                    .ok_or(Status::NotFound)?;
                let SpawnRequest {
                    arguments,
                    environment,
                    standard_input,
                    standard_output,
                    standard_error,
                    home,
                    network,
                    ..
                } = request;
                let launch = ProcessLaunch {
                    host: self,
                    environment,
                    home,
                    network,
                };
                // The spawn future runs on the spawner's task, which is the child's parent.
                let parent = self.tasks.get_current_task_identifier().await;
                let stream = |file: virtual_file_system::File<'static>| {
                    Some(File::new(file.into_synchronous_file()))
                };
                let (input, output, error) = (
                    stream(standard_input),
                    stream(standard_output),
                    stream(standard_error),
                );
                let (state, heap) = run_state(arguments, input, output, error);
                // SAFETY: the bounded C children fit the stack and never unwind; the closure
                // moves every owned value into `run`, as the exit contract requires.
                let spawned = unsafe {
                    stackful::spawn_with_result(
                        self.tasks,
                        parent,
                        &name,
                        stackful::Stack::new(64 * 1024),
                        move || launch.run(state, entry, heap),
                        |status| status as isize,
                    )
                    .await
                }
                .map_err(|error| {
                    Status::from(file_system::IntoIoErrorKind::into_io_error_kind(error))
                })?;
                Ok(spawned)
            })
        }
    }

    /// Entropy source that reads at most three bytes at a time, is pending on every
    /// other read, fills with 0xA5, is unavailable for 13-byte requests, makes
    /// no progress for 17-byte requests and never answers 19-byte requests. It keeps no
    /// per-test mode, so tests that share the process-wide installation cannot
    /// disturb each other.
    struct ScriptedEntropy(portable_atomic::AtomicUsize);
    static ENTROPY: ScriptedEntropy = ScriptedEntropy(portable_atomic::AtomicUsize::new(0));
    impl file_system::DirectBaseOperations for ScriptedEntropy {
        fn read(
            &self,
            cx: &mut core::task::Context<'_>,
            buffer: &mut [u8],
            _: file_system::Size,
        ) -> core::task::Poll<file_system::Result<usize>> {
            match buffer.len() {
                13 => {
                    return core::task::Poll::Ready(Err(
                        file_system::IoErrorKind::DeviceUnavailable,
                    ));
                }
                17 => return core::task::Poll::Ready(Ok(0)),
                19 => return core::task::Poll::Pending,
                _ => {}
            }
            if self
                .0
                .fetch_add(1, portable_atomic::Ordering::Relaxed)
                .is_multiple_of(2)
            {
                cx.waker().wake_by_ref();
                return core::task::Poll::Pending;
            }
            let count = buffer.len().min(3);
            buffer[..count].fill(0xA5);
            core::task::Poll::Ready(Ok(count))
        }
        fn write(
            &self,
            _: &mut core::task::Context<'_>,
            _: &[u8],
            _: file_system::Size,
        ) -> core::task::Poll<file_system::Result<usize>> {
            core::task::Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
        }
    }
    impl file_system::MountOperations for ScriptedEntropy {
        fn unmount(&self) -> file_system::Result<()> {
            Ok(())
        }
    }
    impl file_system::DirectCharacterDevice for ScriptedEntropy {}
    async fn install_entropy() -> &'static virtual_file_system::VirtualFileSystem {
        use virtual_file_system::{ItemStatic, VirtualFileSystem};

        // The task test harness serializes these tests and reuses its manager.
        static VFS: std::sync::OnceLock<&'static VirtualFileSystem> = std::sync::OnceLock::new();
        if VFS.get().is_none() {
            let tasks = task::test_manager();
            let parent = tasks.get_current_task_identifier().await;
            let users = Box::leak(Box::new(users::Manager::new()));
            let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
            let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
            let fs =
                little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
            let vfs = Box::leak(Box::new(VirtualFileSystem::new(tasks, users, time, fs)));
            vfs.create_directory(parent, &"/devices").await.unwrap();
            vfs.create_directory(parent, &"/devices/random")
                .await
                .unwrap();
            vfs.mount_static(
                parent,
                "/devices/random/source",
                ItemStatic::CharacterDevice(&ENTROPY),
            )
            .await
            .unwrap();
            assert!(VFS.set(vfs).is_ok());
        }
        initialize_random(VFS.get().unwrap()).await;
        VFS.get().unwrap()
    }

    #[test]
    fn process_heap_reclaims_c_allocations_on_return() {
        let before = testing::MemoryManager::thread_allocations();
        let mut heap = Heap::new();
        let result = unsafe { run_main(startup_heap_main, &mut heap, &[], None, None, None, None) };
        assert!(
            testing::MemoryManager::thread_allocations()
                .since(before)
                .net_bytes()
                > 0
        );
        drop(heap);
        let delta = testing::MemoryManager::thread_allocations().since(before);
        assert_eq!(result, 0);
        assert!(delta.allocations() >= 2);
        assert_eq!(delta.net_bytes(), 0);
    }

    #[test]
    fn sdk_startup_dispatches_through_supplied_api() {
        let result = unsafe {
            run_main(
                sdk_test_startup,
                &mut Heap::new(),
                &[],
                None,
                None,
                None,
                None,
            )
        };
        assert_eq!(result, 42);
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
                &network::IpCidrAbi::from(network::IpCidr::new_ipv4([127, 0, 0, 1], 8)),
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
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            // Run socket waits on a coroutine while the loopback runner wakes it from
            // its executor thread. The C fixture fits the stack and cannot unwind.
            let result = unsafe {
                stackful::Coroutine::new(stackful::Stack::new(64 * 1024), move || {
                    application_c_network(&XILA_API, &authority, &denied, &listen_only, true)
                })
            }
            .await;
            assert_eq!(result, 0, "network C fixture failed");
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        assert_eq!(
            unsafe { application_c_network(&XILA_API, &authority, &denied, &listen_only, false) },
            0
        );

        install_entropy().await;
        unsafe {
            let mut mono = 0;
            let mut real = 17;
            assert_eq!(xila_time_monotonic(&mut mono), Status::Ok);
            time.invalidate();
            assert_eq!(
                run_main(
                    startup_clock_random_main,
                    &mut Heap::new(),
                    &["unsynchronized"],
                    None,
                    None,
                    None,
                    None
                ),
                0
            );
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
                run_main(
                    startup_clock_random_main,
                    &mut Heap::new(),
                    &["synchronized"],
                    None,
                    None,
                    None,
                    None
                ),
                0
            );
            assert_eq!(
                run_main(
                    startup_empty_main,
                    &mut Heap::new(),
                    &[],
                    None,
                    None,
                    None,
                    None
                ),
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
                    &mut Heap::new(),
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
            assert_eq!(application_c_round_trip(&XILA_API, directory), 0);
            assert_eq!(application_c_file_system(&XILA_API, directory), 0);
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
                Status::BadDescriptor
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
            ReadyEvents, Size, notify_readiness,
        };
        use portable_atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Mutex;

        #[derive(Default)]
        struct ReadState {
            ready: AtomicBool,
            polls: AtomicUsize,
            readiness_seen: AtomicBool,
            control_polls: AtomicUsize,
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
                    return Poll::Ready(Err(file_system::IoErrorKind::Unsupported));
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
                Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
            }

            fn control(
                &self,
                _: &mut DeviceContext,
                cx: &mut TaskContext<'_>,
                command: file_system::ControlCommandIdentifier,
                _: &shared::AnyByLayout,
                output: &mut shared::AnyByLayout,
            ) -> Poll<file_system::Result<()>> {
                use file_system::{ControlCommand, character_device::IS_A_TERMINAL};
                if command != IS_A_TERMINAL::IDENTIFIER {
                    return Poll::Ready(Err(file_system::IoErrorKind::Unsupported));
                }
                if self.0.control_polls.fetch_add(1, Ordering::Relaxed) == 0 {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                *IS_A_TERMINAL::cast_output(output).unwrap() = 1;
                Poll::Ready(Ok(()))
            }

            fn clone_context(&self, _: &DeviceContext) -> file_system::Result<DeviceContext> {
                Ok(DeviceContext::new_empty())
            }

            fn cancel_operation(&self, _: &mut DeviceContext, _: Operation) {
                self.0.waker.lock().unwrap().take();
            }
        }
        impl MountOperations for PendingDevice {}
        impl CharacterDevice for PendingDevice {
            fn readiness(
                &self,
                interests: ReadyEvents,
            ) -> file_system::Result<Option<ReadyEvents>> {
                self.0.readiness_seen.store(true, Ordering::Relaxed);
                Ok(Some(if self.0.ready.load(Ordering::Relaxed) {
                    interests
                } else {
                    ReadyEvents::empty()
                }))
            }
        }

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

        vfs.mount_character_device(
            task,
            "/devices/default",
            ScriptedEntropy(AtomicUsize::new(0)),
        )
        .await
        .unwrap();

        let root = Directory::new(vfs, task, "/", XILA_RIGHT_READ).unwrap();
        let path = c"devices/default";
        let mut default_file = ptr::null_mut();
        unsafe {
            assert_eq!(
                xila_file_open(
                    &root,
                    path.as_ptr(),
                    path.count_bytes(),
                    XILA_OPEN_READ,
                    &mut default_file
                ),
                Status::Ok
            );
            let mut entry = PollEntry {
                handle: default_file.cast(),
                kind: XILA_POLL_FILE,
                events: XILA_POLL_IN | XILA_POLL_OUT,
                revents: 0,
            };
            let mut ready = 0;
            assert_eq!(xila_poll_wait(&mut entry, 1, 0, &mut ready), Status::Ok);
            assert_eq!(ready, 1);
            assert_eq!(entry.revents, XILA_POLL_IN);
            assert_eq!(xila_file_close(default_file), Status::Ok);
        }

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
            let mut terminal = false;
            assert_eq!(
                xila_file_is_terminal(file, &mut terminal),
                Status::WouldBlock
            );
            assert!(!terminal);
        }
        assert_eq!(state.control_polls.swap(0, Ordering::Relaxed), 1);
        assert_eq!(state.polls.swap(0, Ordering::Relaxed), 1);
        assert!(state.waker.lock().unwrap().is_none());

        // SAFETY: The bounded ABI call chain fits within the supplied stack.
        // The fixture closes the handle after both tasks complete.
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
                    let mut terminal = false;
                    let terminal_status = xila_file_is_terminal(file, &mut terminal);
                    i32::from(
                        status != Status::Ok
                            || count != 2
                            || &buffer != b"ok"
                            || terminal_status != Status::Ok
                            || !terminal,
                    )
                },
            )
            .await
            .unwrap()
        };
        // The readiness wait and the read are independently registered on
        // the same handle. A readiness probe never consumes the data.
        let (poller, _) = unsafe {
            stackful::spawn(
                tasks,
                task,
                "ABI poller",
                stackful::Stack::new(64 * 1024),
                move || {
                    let mut entry = PollEntry {
                        handle: file.cast(),
                        kind: XILA_POLL_FILE,
                        events: XILA_POLL_IN,
                        revents: 0,
                    };
                    let mut ready = 0;
                    let status = xila_poll_wait(&mut entry, 1, 1000, &mut ready);
                    i32::from(status != Status::Ok || ready != 1 || entry.revents != XILA_POLL_IN)
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
                    while state.polls.load(Ordering::Relaxed) == 0
                        || !state.readiness_seen.load(Ordering::Relaxed)
                    {
                        task::yield_now().await;
                    }
                    assert_eq!(state.polls.load(Ordering::Relaxed), 1);
                    state.ready.store(true, Ordering::Relaxed);
                    notify_readiness();
                    state.waker.lock().unwrap().take().unwrap().wake();
                },
            )
            .await
            .unwrap();

        assert_eq!(reader.join().await, 0);
        assert_eq!(poller.join().await, 0);
        peer.join().await;
        assert_eq!(unsafe { xila_file_close(file) }, Status::Ok);
        assert_eq!(state.polls.load(Ordering::Relaxed), 2);
        assert_eq!(state.control_polls.load(Ordering::Relaxed), 2);
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn fallback_poll_yields_to_input_producer() {
        use core::task::{Context, Poll};
        use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
        use portable_atomic::{AtomicUsize, Ordering};

        struct Input(&'static AtomicUsize);
        impl DirectBaseOperations for Input {
            fn read(
                &self,
                _: &mut Context<'_>,
                buffer: &mut [u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                if self.0.load(Ordering::Relaxed) == 2 {
                    buffer[0] = b'x';
                    Poll::Ready(Ok(1))
                } else {
                    self.0.store(1, Ordering::Relaxed);
                    Poll::Pending
                }
            }

            fn write(
                &self,
                _: &mut Context<'_>,
                _: &[u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
            }
        }
        impl MountOperations for Input {}
        impl DirectCharacterDevice for Input {}

        let tasks = task::test_manager();
        let parent = tasks.get_current_task_identifier().await;
        let spawner = tasks.get_spawner(parent).await.unwrap();
        let users = Box::leak(Box::new(users::Manager::new()));
        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
        let vfs = Box::leak(Box::new(virtual_file_system::VirtualFileSystem::new(
            tasks, users, time, fs,
        )));
        let input: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
        vfs.mount_character_device(parent, "/input", Input(input))
            .await
            .unwrap();
        let root = Directory::new(vfs, parent, "/", XILA_RIGHT_READ).unwrap();
        let mut file = ptr::null_mut();
        let path = c"input";
        assert_eq!(
            unsafe {
                xila_file_open(
                    &root,
                    path.as_ptr(),
                    path.count_bytes(),
                    XILA_OPEN_READ | XILA_OPEN_NON_BLOCKING,
                    &mut file,
                )
            },
            Status::Ok
        );

        for timeout in [-1, 0] {
            input.store(0, Ordering::Relaxed);
            let (producer, _) = tasks
                .spawn(
                    parent,
                    "input producer",
                    Some(spawner),
                    move |_| async move {
                        while input.load(Ordering::Relaxed) == 0 {
                            task::yield_now().await;
                        }
                        input.store(2, Ordering::Relaxed);
                    },
                )
                .await
                .unwrap();
            // SAFETY: The bounded ABI call chain fits on the stack. The file
            // remains live and this reader is its only user.
            let (reader, _) = unsafe {
                stackful::spawn(
                    tasks,
                    parent,
                    "fallback poll reader",
                    stackful::Stack::new(64 * 1024),
                    move || {
                        let mut entry = PollEntry {
                            handle: file.cast(),
                            kind: XILA_POLL_FILE,
                            events: XILA_POLL_IN,
                            revents: 0,
                        };
                        // Bound retries so starvation fails the test without hanging.
                        for _ in 0..32 {
                            let mut ready = 0;
                            if xila_poll_wait(&mut entry, 1, timeout, &mut ready) != Status::Ok
                                || ready != 1
                                || entry.revents != XILA_POLL_IN
                            {
                                return 1;
                            }
                            let mut byte = 0u8;
                            let mut count = 0;
                            match xila_file_read(file, (&mut byte as *mut u8).cast(), 1, &mut count)
                            {
                                Status::WouldBlock => {}
                                Status::Ok if count == 1 && byte == b'x' => return 0,
                                _ => return 2,
                            }
                        }
                        3
                    },
                )
                .await
                .unwrap()
            };

            assert_eq!(reader.join().await, 0, "fallback poll timeout {timeout}");
            producer.join().await;
        }
        assert_eq!(unsafe { xila_file_close(file) }, Status::Ok);
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn process_exit_cancels_suspended_pipe_poll() {
        use core::{cell::Cell, ffi::c_void};
        use file_system::{StateFlags, notify_readiness, readiness_waiter_count};

        std::thread_local! {
            static READER: Cell<*mut File> = const { Cell::new(ptr::null_mut()) };
        }

        unsafe extern "C" fn wait_for_input(file: *mut c_void) -> *mut c_void {
            let mut entry = PollEntry {
                handle: file,
                kind: XILA_POLL_FILE,
                events: XILA_POLL_IN,
                revents: 0,
            };
            let mut ready = 0;
            unsafe { xila_poll_wait(&mut entry, 1, -1, &mut ready) };
            panic!("poll resumed after process shutdown");
        }

        unsafe extern "C" fn run(startup: *const XilaStartup) -> i32 {
            let process = unsafe { (*startup).process };
            let mut worker = 0;
            unsafe {
                assert_eq!(
                    xila_thread_spawn(
                        process,
                        0,
                        Some(wait_for_input),
                        READER.get().cast(),
                        &mut worker
                    ),
                    Status::Ok
                );
                assert_eq!(xila_thread_yield(), Status::Ok);
                assert_eq!(readiness_waiter_count(), 1);
                assert_eq!(xila_thread_stop_others(process), Status::Ok);
                xila_process_exit(process, 37);
            }
            panic!("process exit returned");
        }

        let tasks = task::test_manager();
        let users = Box::leak(Box::new(users::Manager::new()));
        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
        let vfs = Box::leak(Box::new(virtual_file_system::VirtualFileSystem::new(
            tasks, users, time, fs,
        )));
        let host = Box::leak(Box::new(TestHost { tasks, vfs }));
        let (reader, writer) = vfs
            .create_unnamed_pipe(XILA_PIPE_CAPACITY, StateFlags::empty())
            .await
            .unwrap();
        let reader = File::new(reader.into_synchronous_file()).into_raw();
        READER.set(reader);
        let launch = ProcessLaunch {
            host,
            environment: None,
            home: None,
            network: 0,
        };
        let (state, heap) = run_state(Vec::new(), None, None, None);
        // The bounded probe fits the stack and leaves the pipe endpoints owned
        // by the test so their close notifications cannot clear the hub first.
        let coroutine = unsafe {
            stackful::Coroutine::new(stackful::Stack::new(64 * 1024), move || {
                launch.run(state, run, heap)
            })
        };
        assert_eq!(readiness_waiter_count(), 0);
        assert_eq!(drive_process(coroutine).await, 37);
        assert_eq!(readiness_waiter_count(), 0);
        assert_eq!(notify_readiness(), 0);

        READER.set(ptr::null_mut());
        assert_eq!(unsafe { xila_file_close(reader) }, Status::Ok);
        writer.close(vfs).await.unwrap();
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn stackful_sleep_and_random_yield_to_executor_peer() {
        use portable_atomic::{AtomicBool, AtomicUsize, Ordering};

        install_entropy().await;
        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let spawner = tasks.get_spawner(task).await.unwrap();
        let finished: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let peer_turns: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));

        // SAFETY: The bounded ABI call chain fits within the supplied stack.
        let (sleeper, _) = unsafe {
            stackful::spawn(
                tasks,
                task,
                "ABI sleeper",
                stackful::Stack::new(64 * 1024),
                move || {
                    let start = ::time::MonotonicClock::now(&::time::EmbassyClock);
                    let slept = xila_time_sleep(20_000_000);
                    let elapsed = ::time::MonotonicClock::now(&::time::EmbassyClock) - start;
                    // The entropy source is pending on alternate reads, so this fill
                    // suspends the coroutine as well.
                    let mut buffer = [0u8; 8];
                    let filled = xila_random_fill(buffer.as_mut_ptr().cast(), buffer.len());
                    finished.store(true, Ordering::Relaxed);
                    i32::from(
                        slept != Status::Ok
                            || elapsed < core::time::Duration::from_millis(20)
                            || filled != Status::Ok
                            || buffer != [0xA5; 8],
                    )
                },
            )
            .await
            .unwrap()
        };
        // Runs on the same executor, so it only progresses while the sleeper is suspended.
        let (peer, _) = tasks
            .spawn(task, "sleep peer", Some(spawner), move |_| async move {
                while !finished.load(Ordering::Relaxed) {
                    peer_turns.fetch_add(1, Ordering::Relaxed);
                    task::yield_now().await;
                }
            })
            .await
            .unwrap();
        assert_eq!(sleeper.join().await, 0);
        peer.join().await;
        assert!(peer_turns.load(Ordering::Relaxed) > 1);
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn stackful_sleep_forever_does_not_return() {
        use core::{future::poll_fn, pin::pin, task::Poll};
        use portable_atomic::{AtomicBool, Ordering};

        static RETURNED: AtomicBool = AtomicBool::new(false);
        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let spawner = tasks.get_spawner(task).await.unwrap();
        // The task races the coroutine against a timer and then drops it, so
        // the test ends with no task left behind.
        let (watcher, _) = tasks
            .spawn(
                task,
                "forever sleeper",
                Some(spawner),
                move |_| async move {
                    // SAFETY: The bounded ABI call chain fits within the supplied stack.
                    let coroutine = unsafe {
                        stackful::Coroutine::new(stackful::Stack::new(64 * 1024), || {
                            let status = xila_time_sleep(u64::MAX);
                            RETURNED.store(true, Ordering::Relaxed);
                            i32::from(status != Status::Ok)
                        })
                    };
                    let mut sleeper = pin!(stackful::drive(coroutine));
                    let mut limit = pin!(task::sleep(core::time::Duration::from_millis(50)));
                    poll_fn(|context| {
                        if sleeper.as_mut().poll(context).is_ready() {
                            return Poll::Ready(true);
                        }
                        limit.as_mut().poll(context).map(|()| false)
                    })
                    .await
                },
            )
            .await
            .unwrap();
        assert!(!watcher.join().await, "sleep(u64::MAX) returned");
        assert!(!RETURNED.load(Ordering::Relaxed));
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn stackful_processes_spawn_wait_exit_and_inherit_environment() {
        use file_system::{AccessFlags, CreateFlags, Flags, Path, PathOwned};
        use virtual_file_system::SynchronousFile;

        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let tasks = task::test_manager();
        let users = Box::leak(Box::new(users::Manager::new()));
        let task = tasks.get_current_task_identifier().await;
        let device = file_system::MemoryDevice::<512>::new_static(1024 * 512);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
        let vfs = Box::leak(Box::new(
            virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
        ));
        virtual_file_system::create_default_hierarchy(vfs, task)
            .await
            .unwrap();
        let host: &'static TestHost = Box::leak(Box::new(TestHost { tasks, vfs }));
        let open = || {
            File::new(
                SynchronousFile::open(
                    vfs,
                    task,
                    Path::from_str("/process-output"),
                    Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
                )
                .unwrap(),
            )
        };
        let launch = ProcessLaunch {
            host,
            environment: Some(vec![task::EnvironmentVariable::new("SHARED", "parent")]),
            home: Some(HomeGrant {
                path: PathOwned::new("/".into()).unwrap(),
                rights: 3,
            }),
            network: 0,
        };
        let (input, output, error) = (open(), open(), open());
        let (state, heap) = run_state(Vec::new(), Some(input), Some(output), Some(error));
        // SAFETY: the bounded C parent fits the stack and never unwinds.
        let (parent, _) = unsafe {
            stackful::spawn_with_result(
                tasks,
                task,
                "process parent",
                stackful::Stack::new(128 * 1024),
                move || launch.run(state, process_parent_main, heap),
                |status| status as isize,
            )
            .await
            .unwrap()
        };
        // A nonzero status is the line of the failed check in process.c.
        assert_eq!(parent.join().await, 0);

        let launch = ProcessLaunch {
            host,
            environment: None,
            home: None,
            network: 0,
        };
        let (state, heap) = run_state(Vec::new(), None, None, None);
        let (thread_test, _) = unsafe {
            stackful::spawn_with_driver(
                tasks,
                task,
                "thread test",
                stackful::Stack::new(64 * 1024),
                move || launch.run(state, threads_main, heap),
                drive_process,
                core::convert::identity,
            )
            .await
            .unwrap()
        };
        assert_eq!(thread_test.join().await, 0, "failed check in threads.c");

        // The exiting child's stream was closed (flushed) by exit, and the child's home
        // capability created a file under the root.
        let mut written = [0u8; 3];
        let mut file = SynchronousFile::open(
            vfs,
            task,
            Path::from_str("/exit-output"),
            AccessFlags::Read.into(),
        )
        .unwrap();
        assert_eq!(file.read(&mut written).unwrap(), 3);
        assert_eq!(&written, b"bye");
        vfs.get_statistics_for_task(task, &Path::from_str("/child-home"))
            .await
            .unwrap();
    }

    std::thread_local! {
        /// Start and length of the host state under test.
        static HOST_STATE: core::cell::Cell<(usize, usize)> = const { core::cell::Cell::new((0, 0)) };
    }

    /// Whether `address` lies in the host state, not on the calling coroutine's stack.
    fn in_host_state(address: usize) -> bool {
        let (start, length) = HOST_STATE.get();
        (start..start + length).contains(&address)
    }

    /// The line of the first failed check, so a failure names it.
    macro_rules! check {
        ($condition:expr) => {
            if !$condition {
                return line!() as i32;
            }
        };
    }

    /// The streams `write(2)` and the SDK reach, and the exit path, are in the host state.
    unsafe fn streams_in_host_state(startup: *const XilaStartup) -> i32 {
        let startup = unsafe { &*startup };
        check!(in_host_state(startup as *const XilaStartup as usize));
        check!(in_host_state(startup.process as usize));
        check!(in_host_state(startup.standard_input as usize));
        check!(in_host_state(startup.standard_output as usize));
        check!(in_host_state(startup.standard_error as usize));
        check!(in_host_state(ptr::from_ref(&startup.program_name) as usize));
        check!(startup.program_name.length == b"streams".len());
        check!(!startup.program_name.data.is_null());
        check!(
            unsafe {
                core::slice::from_raw_parts(
                    startup.program_name.data.cast::<u8>(),
                    startup.program_name.length,
                )
            } == b"streams"
        );
        0
    }

    unsafe extern "C" fn streams_return_entry(startup: *const XilaStartup) -> i32 {
        let failed = unsafe { streams_in_host_state(startup) };
        if failed != 0 { failed } else { 6 }
    }

    unsafe extern "C" fn streams_exit_entry(startup: *const XilaStartup) -> i32 {
        let failed = unsafe { streams_in_host_state(startup) };
        if failed != 0 {
            return failed;
        }
        let startup = unsafe { &*startup };
        let api = unsafe { &*startup.api };
        let _ = unsafe { (api.process_exit)(startup.process, 7) };
        1 // Exit returned.
    }

    /// The standard streams of a process live in the host state the loader owns, not in
    /// the coroutine's frames, whether the entry returns or exits early (#766).
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[task::test]
    async fn standard_streams_stay_off_the_coroutine_stack() {
        use file_system::StateFlags;

        let tasks = task::test_manager();
        let task = tasks.get_current_task_identifier().await;
        let users = Box::leak(Box::new(users::Manager::new()));
        let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
        let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
        let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
        let vfs = Box::leak(Box::new(virtual_file_system::VirtualFileSystem::new(
            tasks, users, time, fs,
        )));
        let host = Box::leak(Box::new(TestHost { tasks, vfs }));
        for (entry, expected) in [
            (
                streams_return_entry as unsafe extern "C" fn(*const XilaStartup) -> i32,
                6,
            ),
            (streams_exit_entry, 7),
        ] {
            let (input, kept_input) = vfs
                .create_unnamed_pipe(XILA_PIPE_CAPACITY, StateFlags::empty())
                .await
                .unwrap();
            let (kept_output, output) = vfs
                .create_unnamed_pipe(XILA_PIPE_CAPACITY, StateFlags::empty())
                .await
                .unwrap();
            let (kept_error, error) = vfs
                .create_unnamed_pipe(XILA_PIPE_CAPACITY, StateFlags::empty())
                .await
                .unwrap();
            let stream = |file: virtual_file_system::File<'static>| {
                Some(File::new(file.into_synchronous_file()))
            };
            let (state, heap) = run_state(Vec::new(), stream(input), stream(output), stream(error));
            HOST_STATE.set((state.as_ptr() as usize, core::mem::size_of::<HostState>()));
            let launch = ProcessLaunch {
                host,
                environment: None,
                home: None,
                network: 0,
            };
            // SAFETY: the bounded entries fit the stack and never unwind, and the closure
            // holds only pointers to the state and heap, which are leaked.
            let (handle, _) = unsafe {
                stackful::spawn_with_driver(
                    tasks,
                    task,
                    "streams",
                    stackful::Stack::new(64 * 1024),
                    move || launch.run(state, entry, heap),
                    drive_process,
                    |status| status as isize,
                )
                .await
                .unwrap()
            };
            assert_eq!(handle.join().await, expected, "line of the failed check");
            for kept in [kept_input, kept_output, kept_error] {
                kept.close(vfs).await.unwrap();
            }
        }
    }

    /// A native process that is cancelled while it is suspended: the executor drops its
    /// task, the coroutine's frames are discarded without running anything on them, and the
    /// host releases what the process held from its own state (#731).
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    mod cancellation {
        use super::*;
        use core::{
            future::Future,
            pin::Pin,
            task::{Context as TaskContext, Poll},
        };
        use file_system::{AccessFlags, BaseOperations, CharacterDevice, MountOperations, Size};
        use virtual_file_system::{
            Directory as VfsDirectory, File as VfsFile, ItemStatic, VirtualFileSystem,
        };

        /// A character device whose reads never finish. It is not zero-sized: the file system
        /// tells devices apart by address, which every box of a zero-sized type shares.
        struct Blocked(#[allow(dead_code)] u8);
        impl BaseOperations for Blocked {
            fn read(
                &self,
                _: &mut file_system::Context,
                _: &mut TaskContext<'_>,
                _: &mut [u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                Poll::Pending
            }

            fn write(
                &self,
                _: &mut file_system::Context,
                _: &mut TaskContext<'_>,
                _: &[u8],
                _: Size,
            ) -> Poll<file_system::Result<usize>> {
                Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
            }

            fn clone_context(
                &self,
                _: &file_system::Context,
            ) -> file_system::Result<file_system::Context> {
                Ok(file_system::Context::new_empty())
            }
        }
        impl MountOperations for Blocked {
            fn unmount(&self) -> file_system::Result<()> {
                Ok(())
            }
        }
        impl CharacterDevice for Blocked {}

        type Entry = unsafe extern "C" fn(*const XilaStartup) -> i32;

        struct World {
            task: task::TaskIdentifier,
            vfs: &'static VirtualFileSystem,
            host: &'static TestHost,
        }

        /// A file system with two blocked devices, `/blocked` and `/device`, and a second
        /// file system at `/mnt`.
        async fn world() -> World {
            let tasks = task::test_manager();
            let task = tasks.get_current_task_identifier().await;
            let users = Box::leak(Box::new(users::Manager::new()));
            let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
            let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
            let fs =
                little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
            let vfs = Box::leak(Box::new(VirtualFileSystem::new(tasks, users, time, fs)));
            vfs.mount_character_device(task, "/blocked", Blocked(0))
                .await
                .unwrap();
            vfs.mount_character_device(task, "/device", Blocked(0))
                .await
                .unwrap();
            vfs.create_directory(task, &"/mnt").await.unwrap();
            let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
            let fs =
                little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
            vfs.mount_file_system(fs, "/mnt", task).await.unwrap();
            let host = Box::leak(Box::new(TestHost { tasks, vfs }));
            World { task, vfs, host }
        }

        /// One process, started the way the native loader starts one: its host state and
        /// heap are separate allocations, and the coroutine's entry holds only pointers to
        /// them. Dropping `driver` is the cancellation; the loader then releases the state.
        struct Run {
            state: NonNull<HostState>,
            heap: NonNull<Heap>,
            driver: Option<Pin<Box<dyn Future<Output = i32>>>>,
        }

        impl Run {
            fn new(
                host: &'static TestHost,
                entry: Entry,
                standard_input: Option<File>,
                home: Option<HomeGrant>,
            ) -> Self {
                let (state, heap) = run_state(Vec::new(), standard_input, None, None);
                let launch = ProcessLaunch {
                    host,
                    environment: None,
                    home,
                    network: 0,
                };
                // SAFETY: the bounded entries fit the stack and never unwind, and the closure
                // holds only pointers to the state and heap, which `Run` frees last.
                let coroutine = unsafe {
                    stackful::Coroutine::new(stackful::Stack::new(64 * 1024), move || {
                        launch.run(state, entry, heap)
                    })
                };
                Self {
                    state,
                    heap,
                    driver: Some(Box::pin(drive_process(coroutine))),
                }
            }

            /// Poll the process once, inside the calling task, as the process's own task
            /// would, so that it learns its identity from the real waker.
            async fn poll(&mut self) -> Poll<i32> {
                let driver = self.driver.as_mut().unwrap();
                core::future::poll_fn(|cx| Poll::Ready(driver.as_mut().poll(cx))).await
            }

            fn open_handles(&self) -> usize {
                // SAFETY: the state is live until `Run` is dropped.
                unsafe { self.state.as_ref() }.open_handles()
            }

            /// Drop the process's task future while it is suspended.
            fn cancel(&mut self) {
                self.driver = None;
            }

            /// What the loader does once the coroutine is gone.
            fn release(&self) {
                // SAFETY: the coroutine is gone, or ran to its end.
                unsafe { HostState::release(self.state) };
            }
        }

        impl Drop for Run {
            fn drop(&mut self) {
                self.driver = None;
                // SAFETY: both came from `Box::leak` in `run_state`, and nothing runs now.
                unsafe {
                    HostState::release(self.state);
                    drop(Box::from_raw(self.state.as_ptr()));
                    drop(Box::from_raw(self.heap.as_ptr()));
                }
            }
        }

        fn home() -> Option<HomeGrant> {
            Some(HomeGrant {
                path: file_system::PathOwned::new("/".into()).unwrap(),
                rights: 3,
            })
        }

        fn stream(file: VfsFile<'static>) -> Option<File> {
            Some(File::new(file.into_synchronous_file()))
        }

        macro_rules! check {
            ($condition:expr) => {
                if !$condition {
                    return line!() as i32;
                }
            };
        }

        /// Open every kind of handle the program can own without a network, then do what
        /// `finish` says.
        unsafe fn open_handles(startup: *const XilaStartup) -> i32 {
            let startup = unsafe { &*startup };
            let (mut file, mut directory, mut listing) =
                (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            let (mut reader, mut writer) = (ptr::null_mut(), ptr::null_mut());
            unsafe {
                check!(
                    xila_file_open(
                        startup.home,
                        c"device".as_ptr(),
                        6,
                        XILA_OPEN_READ,
                        &mut file
                    ) == Status::Ok
                );
                check!(
                    xila_directory_open(
                        startup.home,
                        c"mnt".as_ptr(),
                        3,
                        XILA_RIGHT_READ,
                        &mut directory
                    ) == Status::Ok
                );
                check!(
                    xila_directory_iterator_open(directory, c"".as_ptr(), 0, &mut listing)
                        == Status::Ok
                );
                check!(
                    xila_pipe_create(startup.process, 0, &mut reader, &mut writer) == Status::Ok
                );
            }
            0
        }

        /// Opens its handles, then blocks reading its standard input.
        unsafe extern "C" fn open_then_block(startup: *const XilaStartup) -> i32 {
            let failed = unsafe { open_handles(startup) };
            if failed != 0 {
                return failed;
            }
            let (mut byte, mut count) = (0u8, 0);
            let _ = unsafe {
                xila_file_read(
                    (*startup).standard_input,
                    (&raw mut byte).cast(),
                    1,
                    &mut count,
                )
            };
            1 // The read never returns.
        }

        /// Opens its handles and returns without closing them.
        unsafe extern "C" fn open_then_return(startup: *const XilaStartup) -> i32 {
            unsafe { open_handles(startup) }
        }

        /// Opens its handles and exits without closing them.
        unsafe extern "C" fn open_then_exit(startup: *const XilaStartup) -> i32 {
            let failed = unsafe { open_handles(startup) };
            if failed != 0 {
                return failed;
            }
            let _ = unsafe { xila_process_exit((*startup).process, 5) };
            1 // Exit returned.
        }

        /// A worker thread that opens a file and blocks reading it.
        unsafe extern "C" fn worker_opens_then_blocks(
            startup: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void {
            let startup = unsafe { &*startup.cast::<XilaStartup>() };
            let mut file = ptr::null_mut();
            let opened = unsafe {
                xila_file_open(
                    startup.home,
                    c"device".as_ptr(),
                    6,
                    XILA_OPEN_READ,
                    &mut file,
                )
            };
            if opened != Status::Ok {
                return ptr::dangling_mut();
            }
            let (mut byte, mut count) = (0u8, 0);
            let _ = unsafe { xila_file_read(file, (&raw mut byte).cast(), 1, &mut count) };
            ptr::dangling_mut() // The read never returns.
        }

        /// Starts a worker, lets it run, and blocks reading its standard input itself.
        unsafe extern "C" fn main_and_worker_block(startup: *const XilaStartup) -> i32 {
            let mut worker = 0;
            unsafe {
                check!(
                    xila_thread_spawn(
                        (*startup).process,
                        0,
                        Some(worker_opens_then_blocks),
                        startup.cast_mut().cast(),
                        &mut worker,
                    ) == Status::Ok
                );
                check!(xila_thread_yield() == Status::Ok);
                let (mut byte, mut count) = (0u8, 0);
                let _ = xila_file_read(
                    (*startup).standard_input,
                    (&raw mut byte).cast(),
                    1,
                    &mut count,
                );
            }
            1 // The read never returns.
        }

        std::thread_local! {
            /// What `exercise` allocated inside the process: allocations and bytes.
            static MEASURED: core::cell::Cell<(usize, usize)> = const { core::cell::Cell::new((0, 0)) };
        }

        /// Open and close one of each handle that needs no process capability, and report
        /// what that allocated. The calls are the same inside and outside a process: only
        /// inside one are the handles registered.
        unsafe fn exercise(home: *const Directory) -> (usize, usize) {
            let before = testing::MemoryManager::thread_allocations();
            let (mut file, mut directory, mut listing) =
                (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            unsafe {
                assert_eq!(
                    xila_file_open(home, c"device".as_ptr(), 6, XILA_OPEN_READ, &mut file),
                    Status::Ok
                );
                assert_eq!(
                    xila_directory_open(home, c"mnt".as_ptr(), 3, XILA_RIGHT_READ, &mut directory),
                    Status::Ok
                );
                assert_eq!(
                    xila_directory_iterator_open(directory, c"".as_ptr(), 0, &mut listing),
                    Status::Ok
                );
                assert_eq!(xila_directory_iterator_close(listing), Status::Ok);
                assert_eq!(xila_directory_close(directory), Status::Ok);
                assert_eq!(xila_file_close(file), Status::Ok);
            }
            let delta = testing::MemoryManager::thread_allocations().since(before);
            assert_eq!(delta.net_bytes(), 0);
            (delta.allocations(), delta.allocated_bytes())
        }

        unsafe extern "C" fn measure_in_process(startup: *const XilaStartup) -> i32 {
            // The first round settles anything the calls initialize once.
            unsafe { exercise((*startup).home) };
            MEASURED.set(unsafe { exercise((*startup).home) });
            0
        }

        /// Blocks in a random fill that never finishes.
        unsafe extern "C" fn block_in_random_fill(_: *const XilaStartup) -> i32 {
            let mut bytes = [0u8; 19];
            let _ = unsafe { xila_random_fill(bytes.as_mut_ptr().cast(), bytes.len()) };
            1 // The fill never returns.
        }

        async fn busy(vfs: &VirtualFileSystem, path: &str, when: &str) {
            assert!(
                vfs.unmount(path, false).await.is_err(),
                "{path} should still be in use {when}"
            );
        }

        async fn unmounts(vfs: &VirtualFileSystem, path: &str, when: &str) {
            let result = vfs.unmount(path, false).await;
            assert!(
                result.is_ok(),
                "{path} should be free to unmount {when}: {result:?}"
            );
        }

        /// The device handle a test keeps beside a process's: unmounting stays refused until
        /// the test closes it, which a process closing its own twice would break.
        async fn keep(world: &World, path: &str) -> VfsFile<'static> {
            VfsFile::open(world.vfs, world.task, path, AccessFlags::Read.into())
                .await
                .unwrap()
        }

        #[task::test]
        async fn cancelled_process_releases_its_streams_and_handles() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            let input = VfsFile::open(vfs, task, "/blocked", AccessFlags::Read.into())
                .await
                .unwrap();
            let mut run = Run::new(world.host, open_then_block, stream(input), home());
            // The process opens a file, a directory, a listing and a pipe, and blocks on a read
            // of its standard input.
            assert_eq!(run.poll().await, Poll::Pending);
            assert_eq!(run.open_handles(), 5, "file, directory, listing, pipe ends");
            for path in ["/blocked", "/device", "/mnt"] {
                busy(vfs, path, "(line 287)").await;
            }

            // The test keeps one handle of each beside the process's.
            let kept_input = keep(&world, "/blocked").await;
            let kept_device = keep(&world, "/device").await;
            let kept_listing = VfsDirectory::open(vfs, task, "/mnt").await.unwrap();

            run.cancel();
            run.release();
            assert_eq!(run.open_handles(), 0);
            // The process's handles are closed, and only once: the test's still hold each.
            for path in ["/blocked", "/device", "/mnt"] {
                busy(vfs, path, "(line 300)").await;
            }
            kept_input.close(vfs).await.unwrap();
            kept_device.close(vfs).await.unwrap();
            kept_listing.close(vfs).await.unwrap();
            for path in ["/blocked", "/device", "/mnt"] {
                unmounts(vfs, path, "(line 306)").await;
            }
            // Releasing again does nothing.
            run.release();
        }

        #[task::test]
        async fn process_cancelled_before_its_first_poll_releases_its_streams() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            let input = VfsFile::open(vfs, task, "/blocked", AccessFlags::Read.into())
                .await
                .unwrap();
            let mut run = Run::new(world.host, open_then_block, stream(input), home());
            let kept = keep(&world, "/blocked").await;
            busy(vfs, "/blocked", "(line 321)").await;
            // The coroutine never started: its entry is dropped unrun, with the launch request.
            run.cancel();
            assert_eq!(run.open_handles(), 0);
            run.release();
            busy(vfs, "/blocked", "(line 326)").await;
            kept.close(vfs).await.unwrap();
            unmounts(vfs, "/blocked", "(line 328)").await;
        }

        #[task::test]
        async fn cancel_inside_a_random_fill_closes_the_entropy_source() {
            let world = world().await;
            let vfs = install_entropy().await;
            let source = "/devices/random/source";
            let mut run = Run::new(world.host, block_in_random_fill, None, None);
            assert_eq!(run.poll().await, Poll::Pending);
            // The fill holds the source open while it waits.
            busy(vfs, source, "(line 339)").await;
            let kept = VfsFile::open(vfs, world.task, source, AccessFlags::Read.into())
                .await
                .unwrap();
            run.cancel();
            run.release();
            // Closed once: only the test's own handle keeps it in use.
            busy(vfs, source, "(line 346)").await;
            kept.close(vfs).await.unwrap();
            unmounts(vfs, source, "(line 348)").await;
            // Put the shared device back for the other tests.
            vfs.mount_static(world.task, source, ItemStatic::CharacterDevice(&ENTROPY))
                .await
                .unwrap();
        }

        /// The handles a worker thread opened belong to the process, so a cancellation closes
        /// them too, and the worker's stack goes with the main one.
        #[task::test]
        async fn cancel_closes_the_handles_of_worker_threads_and_frees_their_stacks() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            let start = async || {
                let input = VfsFile::open(vfs, task, "/blocked", AccessFlags::Read.into())
                    .await
                    .unwrap();
                Run::new(world.host, main_and_worker_block, stream(input), home())
            };
            // The first round grows the task manager's retained pool.
            let mut warm = start().await;
            assert!(warm.poll().await.is_pending());
            warm.cancel();
            drop(warm);

            let before = testing::MemoryManager::thread_allocations();
            let mut run = start().await;
            assert!(run.poll().await.is_pending());
            assert_eq!(run.open_handles(), 1, "the worker's file");
            let kept = keep(&world, "/device").await;
            busy(vfs, "/device", "while the worker holds its file").await;
            run.cancel();
            run.release();
            assert_eq!(run.open_handles(), 0);
            busy(vfs, "/device", "with only the test's handle open").await;
            drop(run);
            let delta = testing::MemoryManager::thread_allocations().since(before);
            kept.close(vfs).await.unwrap();
            assert_eq!(
                delta.net_bytes(),
                0,
                "both stacks and every handle are freed"
            );
            unmounts(vfs, "/device", "once the test's handle is closed").await;
        }

        /// Registering a handle costs no allocation: the same calls allocate the same count and
        /// bytes inside a process, where the handles are registered, as outside one.
        #[task::test]
        async fn tracking_handles_allocates_nothing() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            let mut run = Run::new(world.host, measure_in_process, None, home());
            assert_eq!(run.poll().await, Poll::Ready(0));
            drop(run);
            let inside = MEASURED.get();

            let home = Directory::new(vfs, task, "/", XILA_RIGHT_READ | XILA_RIGHT_WRITE).unwrap();
            unsafe { exercise(&home) };
            let outside = unsafe { exercise(&home) };
            assert_eq!(inside, outside);
            assert!(inside.0 > 0, "the calls allocate their objects");
        }

        /// A process that ends by returning or by exiting closes the handles it forgot, once.
        #[task::test]
        async fn handles_left_open_at_exit_are_closed_once() {
            for (entry, expected) in [(open_then_return as Entry, 0), (open_then_exit as Entry, 5)]
            {
                let world = world().await;
                let World { vfs, .. } = world;
                let mut run = Run::new(world.host, entry, None, home());
                let kept_device = keep(&world, "/device").await;
                let kept_listing = VfsDirectory::open(vfs, world.task, "/mnt").await.unwrap();
                assert_eq!(run.poll().await, Poll::Ready(expected));
                assert_eq!(run.open_handles(), 0);
                busy(vfs, "/device", "(line 373)").await;
                busy(vfs, "/mnt", "(line 374)").await;
                // Releasing a process that ended changes nothing.
                run.release();
                busy(vfs, "/device", "(line 377)").await;
                busy(vfs, "/mnt", "(line 378)").await;
                kept_device.close(vfs).await.unwrap();
                kept_listing.close(vfs).await.unwrap();
                unmounts(vfs, "/device", "(line 381)").await;
                unmounts(vfs, "/mnt", "(line 382)").await;
            }
        }

        std::thread_local! {
            /// The file system whose namespace lock the lock holders below take, and the
            /// task that opens directories in it.
            static HELD: core::cell::Cell<Option<(&'static VirtualFileSystem, task::TaskIdentifier)>> =
                const { core::cell::Cell::new(None) };
        }

        /// Suspend with the VFS namespace lock held, as a path call does while it waits for
        /// the task or user managers, until the thread is stopped.
        fn hold_namespace_lock() {
            let (vfs, _) = HELD.get().expect("set by the test");
            stackful::suspend_on_cancellable(
                vfs.hold_namespace_lock(core::future::pending::<()>()),
            );
        }

        unsafe extern "C" fn worker_holds_lock(
            _: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void {
            hold_namespace_lock();
            ptr::dangling_mut() // The hold never ends.
        }

        /// Opens its handles, starts a worker that takes the namespace lock and lets it
        /// run, so that the lock is held when main ends.
        unsafe fn open_then_let_a_worker_hold_the_lock(startup: *const XilaStartup) -> i32 {
            let failed = unsafe { open_handles(startup) };
            if failed != 0 {
                return failed;
            }
            let mut worker = 0;
            unsafe {
                check!(
                    xila_thread_spawn(
                        (*startup).process,
                        0,
                        Some(worker_holds_lock),
                        ptr::null_mut(),
                        &mut worker,
                    ) == Status::Ok
                );
                check!(xila_thread_yield() == Status::Ok);
            }
            0
        }

        unsafe extern "C" fn return_while_a_worker_holds_the_lock(
            startup: *const XilaStartup,
        ) -> i32 {
            unsafe { open_then_let_a_worker_hold_the_lock(startup) }
        }

        unsafe extern "C" fn exit_while_a_worker_holds_the_lock(
            startup: *const XilaStartup,
        ) -> i32 {
            let failed = unsafe { open_then_let_a_worker_hold_the_lock(startup) };
            if failed != 0 {
                return failed;
            }
            let _ = unsafe { xila_process_exit((*startup).process, 5) };
            1 // Exit returned.
        }

        unsafe extern "C" fn worker_exits(
            startup: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void {
            let startup = startup.cast::<XilaStartup>();
            let _ = unsafe { xila_process_exit((*startup).process, 7) };
            ptr::dangling_mut() // Exit returned.
        }

        /// Opens its handles, starts a worker that exits the process, and takes the
        /// namespace lock itself before the worker runs.
        unsafe extern "C" fn worker_exits_while_main_holds_the_lock(
            startup: *const XilaStartup,
        ) -> i32 {
            let failed = unsafe { open_handles(startup) };
            if failed != 0 {
                return failed;
            }
            let mut worker = 0;
            unsafe {
                check!(
                    xila_thread_spawn(
                        (*startup).process,
                        0,
                        Some(worker_exits),
                        startup.cast_mut().cast(),
                        &mut worker,
                    ) == Status::Ok
                );
            }
            hold_namespace_lock();
            1 // The hold never ends.
        }

        /// A process that ends while another of its threads is suspended with the VFS
        /// namespace lock held stops that thread, which gives the lock back, before it
        /// closes what it holds: closing its listing and directory needs that lock.
        #[task::test]
        async fn ending_does_not_wait_for_a_lock_a_stopped_thread_held() {
            for (entry, expected, how) in [
                (
                    return_while_a_worker_holds_the_lock as Entry,
                    0,
                    "main returns",
                ),
                (exit_while_a_worker_holds_the_lock as Entry, 5, "main exits"),
                (
                    worker_exits_while_main_holds_the_lock as Entry,
                    7,
                    "a worker exits",
                ),
            ] {
                let world = world().await;
                let World { vfs, .. } = world;
                HELD.set(Some((vfs, world.task)));
                let mut run = Run::new(world.host, entry, None, home());
                let mut ended = Poll::Pending;
                for _ in 0..100 {
                    ended = run.poll().await;
                    if ended.is_ready() {
                        break;
                    }
                }
                assert_eq!(ended, Poll::Ready(expected), "{how}: the process must end");
                assert_eq!(run.open_handles(), 0, "{how}");
                // The lock is free and the process's handles are closed.
                unmounts(vfs, "/mnt", how).await;
                drop(run);
            }
        }

        /// A worker whose cancellable wait owns an open directory, as a directory open does
        /// between its open and its close.
        unsafe extern "C" fn worker_holds_a_directory(
            _: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void {
            let (vfs, task) = HELD.get().expect("set by the test");
            stackful::suspend_on_cancellable(async move {
                let directory = VfsDirectory::open(vfs, task, "/mnt").await;
                core::future::pending::<()>().await;
                drop(directory);
            });
            ptr::dangling_mut() // The wait never ends.
        }

        /// Starts that worker, lets it open its directory, and returns.
        unsafe extern "C" fn return_while_a_worker_holds_a_directory(
            startup: *const XilaStartup,
        ) -> i32 {
            let mut worker = 0;
            unsafe {
                check!(
                    xila_thread_spawn(
                        (*startup).process,
                        0,
                        Some(worker_holds_a_directory),
                        ptr::null_mut(),
                        &mut worker,
                    ) == Status::Ok
                );
                check!(xila_thread_yield() == Status::Ok);
            }
            0
        }

        /// Stopping a worker drops its wait, and so closes the directory the wait owns,
        /// without suspending the thread that ends the process, even while another task
        /// holds the namespace lock the close counts the directory under. The process ends
        /// under that lock, so no cancellation can catch its cleanup halfway; the directory
        /// is given back once, when the lock is next taken, and the worker's coroutine is
        /// freed with the process.
        #[task::test]
        async fn a_stopped_peer_closes_its_directory_while_the_lock_is_held() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            HELD.set(Some((vfs, task)));
            let run_once = async || {
                let mut run = Run::new(
                    world.host,
                    return_while_a_worker_holds_a_directory,
                    None,
                    home(),
                );
                // The worker opens its directory, then main returns while the test holds
                // the namespace lock, which closing that directory would wait for.
                let mut ended = run.poll().await;
                if ended.is_pending() {
                    ended = vfs
                        .hold_namespace_lock(async {
                            let mut ended = Poll::Pending;
                            for _ in 0..100 {
                                ended = run.poll().await;
                                if ended.is_ready() {
                                    break;
                                }
                            }
                            ended
                        })
                        .await;
                }
                // What a host does with a process that has not ended: cancel it.
                if ended.is_pending() {
                    run.cancel();
                }
                drop(run);
                // A close takes the lock and gives back what was owed.
                VfsDirectory::open(vfs, task, "/")
                    .await
                    .unwrap()
                    .close(vfs)
                    .await
                    .unwrap();
                ended
            };
            // The first round grows the task manager's retained pool.
            assert_eq!(run_once().await, Poll::Ready(0));
            let before = testing::MemoryManager::thread_allocations();
            let ended = run_once().await;
            let delta = testing::MemoryManager::thread_allocations().since(before);
            assert_eq!(
                ended,
                Poll::Ready(0),
                "the process ends under the held lock"
            );
            assert_eq!(
                delta.net_bytes(),
                0,
                "the worker's coroutine and the owed reference are freed"
            );
            unmounts(
                vfs,
                "/mnt",
                "once every directory the worker opened is closed",
            )
            .await;
        }

        /// Every way a process can end, cancellation included, gives back every byte it took:
        /// its stack, its state, its handles and its streams.
        #[task::test]
        async fn every_ending_returns_all_memory() {
            let world = world().await;
            let World { vfs, task, .. } = world;
            for _ in 0..2 {
                // The first rounds grow the task manager's retained pool.
                for entry in [open_then_return, open_then_exit] {
                    let mut run = Run::new(world.host, entry, None, home());
                    assert!(run.poll().await.is_ready());
                }
                let input = VfsFile::open(vfs, task, "/blocked", AccessFlags::Read.into())
                    .await
                    .unwrap();
                let mut run = Run::new(world.host, open_then_block, stream(input), home());
                assert!(run.poll().await.is_pending());
                run.cancel();
            }
            let mut counts = Vec::new();
            for entry in [open_then_return, open_then_exit] {
                let before = testing::MemoryManager::thread_allocations();
                let mut run = Run::new(world.host, entry, None, home());
                assert!(run.poll().await.is_ready());
                drop(run);
                let delta = testing::MemoryManager::thread_allocations().since(before);
                assert_eq!(delta.net_bytes(), 0, "a process that ends leaves nothing");
                counts.push(delta.allocations());
            }
            let before = testing::MemoryManager::thread_allocations();
            let input = VfsFile::open(vfs, task, "/blocked", AccessFlags::Read.into())
                .await
                .unwrap();
            let mut run = Run::new(world.host, open_then_block, stream(input), home());
            assert!(run.poll().await.is_pending());
            run.cancel();
            drop(run);
            let delta = testing::MemoryManager::thread_allocations().since(before);
            assert_eq!(
                delta.net_bytes(),
                0,
                "a cancelled process leaves nothing, its stack included"
            );
            // Returning and exiting allocate the same: tracking handles costs no allocation.
            assert_eq!(counts[0], counts[1]);
        }
    }
}
