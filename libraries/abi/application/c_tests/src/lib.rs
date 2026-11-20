#[cfg(test)]
mod tests {
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
        fn application_c_round_trip(api: *const XilaApi, directory: *const Directory) -> i32;
        fn application_c_file_system(api: *const XilaApi, directory: *const Directory) -> i32;
        fn startup_test_main(startup: *const XilaStartup) -> i32;
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
        ) -> i32;
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
                    ..
                } = request;
                let launch = ProcessLaunch {
                    host: self,
                    environment,
                    home,
                };
                // The spawn future runs on the spawner's task, which is the child's parent.
                let parent = self.tasks.get_current_task_identifier().await;
                let heap: &'static mut Heap = Box::leak(Box::new(Heap::new()));
                let stream = |file: virtual_file_system::File<'static>| {
                    Some(File::new(file.into_synchronous_file()))
                };
                let (input, output, error) = (
                    stream(standard_input),
                    stream(standard_output),
                    stream(standard_error),
                );
                // SAFETY: the bounded C children fit the stack and never unwind; the closure
                // moves every owned value into `run`, as the exit contract requires.
                let (handle, _) = unsafe {
                    stackful::spawn_with_result(
                        self.tasks,
                        parent,
                        &name,
                        stackful::Stack::new(64 * 1024),
                        move || launch.run(entry, heap, arguments, input, output, error),
                        |status| status as isize,
                    )
                    .await
                }
                .map_err(|error| {
                    Status::from(file_system::IntoIoErrorKind::into_io_error_kind(error))
                })?;
                Ok(handle)
            })
        }
    }

    /// Entropy source that reads at most three bytes at a time, is pending on every
    /// other read, fills with 0xA5, is unavailable for 13-byte requests and makes
    /// no progress for 17-byte requests. It keeps no per-test mode, so tests that
    /// share the process-wide installation cannot disturb each other.
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
    impl file_system::MountOperations for ScriptedEntropy {}
    impl file_system::DirectCharacterDevice for ScriptedEntropy {}
    async fn install_entropy() {
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
            assert_eq!(
                application_c_network(&XILA_API, &authority, &denied, &listen_only),
                0
            );
        }

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
        };
        let heap: &'static mut Heap = Box::leak(Box::new(Heap::new()));
        let (input, output, error) = (open(), open(), open());
        // SAFETY: the bounded C parent fits the stack and never unwinds.
        let (parent, _) = unsafe {
            stackful::spawn_with_result(
                tasks,
                task,
                "process parent",
                stackful::Stack::new(128 * 1024),
                move || {
                    launch.run(
                        process_parent_main,
                        heap,
                        Vec::new(),
                        Some(input),
                        Some(output),
                        Some(error),
                    )
                },
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
        };
        let (thread_test, _) = unsafe {
            stackful::spawn_with_driver(
                tasks,
                task,
                "thread test",
                stackful::Stack::new(64 * 1024),
                move || launch.run(threads_main, &mut Heap::new(), Vec::new(), None, None, None),
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
}
