#![cfg(all(target_os = "linux", feature = "real-time-clock"))]

//! Real-time clock device node: a `MockRealTimeClock` controller is mounted as
//! `/devices/time/real_time_clock` and driven through the VFS `File` API and
//! the `GET_TIME` / `SET_TIME` control commands. The `rtcc` adapter is tested
//! with a mock `DateTimeAccess` chip.

extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use drivers_peripherals::real_time_clock::{
    GET_TIME, RealTimeClockTimeAbi, SET_TIME, mount_real_time_clock,
};
use file_system::{AccessFlags, Flags, IoErrorKind, MemoryDevice};
use peripherals::real_time_clock::{MockRealTimeClock, RealTimeClockController, Time};
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

use drivers_peripherals::real_time_clock::ABI_VERSION;

fn leaked_mock(time: Option<Time>) -> &'static MockRealTimeClock {
    Box::leak(Box::new(MockRealTimeClock::new(time)))
}

async fn initialize() -> (
    TaskIdentifier,
    &'static VirtualFileSystem,
    &'static task::Manager,
    &'static users::Manager,
) {
    let time_manager = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let task_instance = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));

    if !log::is_initialized() {
        log::initialize(&testing::Logger).unwrap();
    }

    let task = task_instance.get_current_task_identifier().await;

    let device = MemoryDevice::<512>::new_static(1024 * 512);
    let options = little_fs::Options::new(256);
    little_fs::FileSystem::format(device, options).unwrap();
    let file_system = little_fs::FileSystem::new(device, options).unwrap();

    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(task_instance, users_manager, time_manager, file_system)
            .unwrap(),
    ));
    // Like the firmware, `/devices` is an in-memory file system whose entries
    // carry permissions, and the default hierarchy sets `/` and the device
    // directories to the default directory permissions.
    virtual_file_system
        .mount_file_system(
            memory_file_system::FileSystem::new(),
            file_system::Path::DEVICES,
            task,
        )
        .await
        .unwrap();
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();

    (task, virtual_file_system, task_instance, users_manager)
}

async fn open<'a>(
    vfs: &'a VirtualFileSystem,
    task: TaskIdentifier,
    access: AccessFlags,
) -> virtual_file_system::Result<File<'a>> {
    File::open(
        vfs,
        task,
        "/devices/time/real_time_clock",
        Flags::new(access, None, None),
    )
    .await
}

fn request(seconds: i64, nanoseconds: u32) -> RealTimeClockTimeAbi {
    RealTimeClockTimeAbi {
        version: ABI_VERSION,
        valid: 1,
        reserved: 0,
        nanoseconds,
        seconds,
    }
}

fn query() -> RealTimeClockTimeAbi {
    RealTimeClockTimeAbi {
        version: ABI_VERSION,
        valid: 0,
        reserved: 0,
        nanoseconds: 0,
        seconds: 0,
    }
}

#[task::test]
async fn get_time_returns_the_time_the_clock_holds() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(Some(Time::new(1_767_225_600, 123_456_789)));
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let mut file = open(vfs, task, AccessFlags::Read).await.unwrap();
    let time: RealTimeClockTimeAbi = file.control(GET_TIME, &query()).await.unwrap();
    assert_eq!(time.version, ABI_VERSION);
    assert_eq!(time.valid, 1);
    assert_eq!(time.reserved, 0);
    assert_eq!(time.seconds, 1_767_225_600);
    assert_eq!(time.nanoseconds, 123_456_789);
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn get_time_reports_no_valid_time() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(None);
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let mut file = open(vfs, task, AccessFlags::Read).await.unwrap();
    let time: RealTimeClockTimeAbi = file.control(GET_TIME, &query()).await.unwrap();
    assert_eq!(time.valid, 0);
    assert_eq!(time.seconds, 0);
    assert_eq!(time.nanoseconds, 0);
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn set_time_writes_and_reads_back() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(None);
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let mut file = open(vfs, task, AccessFlags::READ_WRITE).await.unwrap();
    let written: RealTimeClockTimeAbi = file
        .control(SET_TIME, &request(-1, 999_999_999))
        .await
        .unwrap();
    assert_eq!(written.valid, 1);
    assert_eq!(written.seconds, -1);
    assert_eq!(written.nanoseconds, 999_999_999);
    assert_eq!(mock.time(), Some(Time::new(-1, 999_999_999)));
    assert_eq!(mock.write_count(), 1);

    let time: RealTimeClockTimeAbi = file.control(GET_TIME, &query()).await.unwrap();
    assert_eq!((time.seconds, time.nanoseconds), (-1, 999_999_999));
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn invalid_payloads_are_rejected() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(Some(Time::new(42, 0)));
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let mut file = open(vfs, task, AccessFlags::READ_WRITE).await.unwrap();

    // Wrong version, non-zero reserved, invalid validity and out-of-range nanoseconds.
    let cases = [
        RealTimeClockTimeAbi {
            version: ABI_VERSION + 1,
            ..request(10, 0)
        },
        RealTimeClockTimeAbi {
            reserved: 1,
            ..request(10, 0)
        },
        RealTimeClockTimeAbi {
            valid: 0,
            ..request(10, 0)
        },
        RealTimeClockTimeAbi {
            nanoseconds: 1_000_000_000,
            ..request(10, 0)
        },
    ];
    for case in cases {
        assert_eq!(
            file.control(SET_TIME, &case).await.err(),
            Some(IoErrorKind::InvalidArgument)
        );
    }
    // GET_TIME checks version and reserved of its input too.
    assert_eq!(
        file.control(
            GET_TIME,
            &RealTimeClockTimeAbi {
                version: 2,
                ..query()
            }
        )
        .await
        .err(),
        Some(IoErrorKind::InvalidArgument)
    );
    assert_eq!(
        file.control(
            GET_TIME,
            &RealTimeClockTimeAbi {
                reserved: 9,
                ..query()
            }
        )
        .await
        .err(),
        Some(IoErrorKind::InvalidArgument)
    );
    // Nothing was written by the rejected commands.
    assert_eq!(mock.time(), Some(Time::new(42, 0)));
    assert_eq!(mock.write_count(), 0);
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn plain_read_and_write_and_unknown_commands_are_unsupported() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(Some(Time::new(7, 8)));
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let mut file = open(vfs, task, AccessFlags::READ_WRITE).await.unwrap();
    let mut buffer = [0u8; 4];
    assert_eq!(
        file.read(&mut buffer).await.err(),
        Some(IoErrorKind::Unsupported)
    );
    assert_eq!(
        file.write(&buffer).await.err(),
        Some(IoErrorKind::Unsupported)
    );
    // A command of another kind byte is unknown to this node.
    assert_eq!(
        file.control(device::power::GET_CAPABILITIES, &())
            .await
            .err(),
        Some(IoErrorKind::Unsupported)
    );
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn controller_failures_map_to_io_errors() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(None);
    mount_real_time_clock(vfs, task, mock).await.unwrap();
    mock.fail_reads(true);

    let mut file = open(vfs, task, AccessFlags::Read).await.unwrap();
    assert_eq!(
        file.control(GET_TIME, &query()).await.err(),
        Some(IoErrorKind::InputOutput)
    );
    file.close(vfs).await.unwrap();

    mock.fail_reads(false);
    mock.fail_writes(true);
    let mut file = open(vfs, task, AccessFlags::READ_WRITE).await.unwrap();
    assert_eq!(
        file.control(SET_TIME, &request(1, 0)).await.err(),
        Some(IoErrorKind::InputOutput)
    );
    assert_eq!(mock.write_count(), 0);
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn second_open_of_the_node_is_busy() {
    let (task, vfs, _, _) = initialize().await;
    let mock = leaked_mock(None);
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    let file = open(vfs, task, AccessFlags::Read).await.unwrap();
    assert_eq!(
        open(vfs, task, AccessFlags::Read).await.err(),
        Some(IoErrorKind::ResourceBusy)
    );
    file.close(vfs).await.unwrap();
    open(vfs, task, AccessFlags::Read).await.unwrap();
}

#[task::test]
async fn writes_are_reserved_to_root_but_reads_are_open_to_all() {
    let (task, vfs, task_manager, users_manager) = initialize().await;
    let mock = leaked_mock(Some(Time::new(5, 6)));
    mount_real_time_clock(vfs, task, mock).await.unwrap();

    // A non-root user in its own group.
    let group = users_manager.get_new_group_identifier().await.unwrap();
    users_manager.add_group(group, "guests", &[]).await.unwrap();
    let user = users_manager.get_new_user_identifier().await.unwrap();
    users_manager
        .add_user(user, "guest", group, "/users/guest")
        .await
        .unwrap();
    task_manager.set_user(task, user).await.unwrap();

    // Read-only open works and GET_TIME is available.
    let mut file = open(vfs, task, AccessFlags::Read).await.unwrap();
    let time: RealTimeClockTimeAbi = file.control(GET_TIME, &query()).await.unwrap();
    assert_eq!((time.valid, time.seconds, time.nanoseconds), (1, 5, 6));
    // A write-direction control command on a read-only handle is refused.
    assert_eq!(
        file.control(SET_TIME, &request(9, 9)).await.err(),
        Some(IoErrorKind::BadDescriptor)
    );
    file.close(vfs).await.unwrap();

    // Opening for write is denied.
    assert_eq!(
        open(vfs, task, AccessFlags::Write).await.err(),
        Some(IoErrorKind::PermissionDenied)
    );
    assert_eq!(
        open(vfs, task, AccessFlags::READ_WRITE).await.err(),
        Some(IoErrorKind::PermissionDenied)
    );

    // Root can open for write and set the time.
    task_manager
        .set_user(task, users::UserIdentifier::ROOT)
        .await
        .unwrap();
    let mut file = open(vfs, task, AccessFlags::READ_WRITE).await.unwrap();
    file.control(SET_TIME, &request(9, 9)).await.unwrap();
    assert_eq!(mock.time(), Some(Time::new(9, 9)));
    file.close(vfs).await.unwrap();
}

#[cfg(feature = "external-real-time-clock")]
#[allow(
    clippy::disallowed_types,
    reason = "host-only test shares the mock chip through std Arc and Mutex"
)]
mod external {
    //! The `rtcc` adapter with a mock `DateTimeAccess` chip.
    use super::*;
    use drivers_peripherals::real_time_clock::ExternalRealTimeClock;
    use peripherals::real_time_clock::Error;
    use rtcc::{DateTimeAccess, NaiveDate, NaiveDateTime};
    use std::sync::{Arc, Mutex as StdMutex};

    fn naive(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        nanos: u32,
    ) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_nano_opt(hour, minute, second, nanos)
            .unwrap()
    }

    struct MockChip {
        datetime: Arc<StdMutex<NaiveDateTime>>,
        fail: bool,
    }

    impl MockChip {
        fn new(datetime: NaiveDateTime, fail: bool) -> (Self, Arc<StdMutex<NaiveDateTime>>) {
            let shared = Arc::new(StdMutex::new(datetime));
            (
                Self {
                    datetime: shared.clone(),
                    fail,
                },
                shared,
            )
        }
    }

    impl DateTimeAccess for MockChip {
        type Error = ();
        fn datetime(&mut self) -> Result<NaiveDateTime, ()> {
            if self.fail {
                return Err(());
            }
            Ok(*self.datetime.lock().unwrap())
        }
        fn set_datetime(&mut self, datetime: &NaiveDateTime) -> Result<(), ()> {
            if self.fail {
                return Err(());
            }
            *self.datetime.lock().unwrap() = *datetime;
            Ok(())
        }
    }

    #[test]
    fn reads_convert_naive_date_time_as_utc() {
        let (chip, _) = MockChip::new(naive(2026, 1, 1, 12, 34, 56, 789_000_000), false);
        let clock = ExternalRealTimeClock::new(chip);
        let time = clock.read().unwrap().unwrap();
        // 2026-01-01T12:34:56.789Z is 1767270896.789 Unix seconds.
        assert_eq!(time, Time::new(1_767_270_896, 789_000_000));
    }

    #[test]
    fn writes_convert_unix_time_to_naive_date_time() {
        let (chip, stored) = MockChip::new(naive(1970, 1, 1, 0, 0, 0, 0), false);
        let clock = ExternalRealTimeClock::new(chip);
        clock.write(Time::new(-1, 500_000_000)).unwrap();
        assert_eq!(
            *stored.lock().unwrap(),
            naive(1969, 12, 31, 23, 59, 59, 500_000_000)
        );
        // And reads back the same instant.
        assert_eq!(clock.read().unwrap(), Some(Time::new(-1, 500_000_000)));
    }

    #[test]
    fn out_of_range_and_failing_chips_report_errors() {
        let (chip, _) = MockChip::new(naive(1970, 1, 1, 0, 0, 0, 0), false);
        let clock = ExternalRealTimeClock::new(chip);
        // Beyond chrono's date range (about +/-262,000 years).
        assert_eq!(
            clock.write(Time::new(i64::MAX, 0)).unwrap_err(),
            Error::Invalid
        );
        let (failing_chip, _) = MockChip::new(naive(1970, 1, 1, 0, 0, 0, 0), true);
        let failing = ExternalRealTimeClock::new(failing_chip);
        assert_eq!(failing.read().unwrap_err(), Error::Other);
        assert_eq!(failing.write(Time::new(0, 0)).unwrap_err(), Error::Other);
    }
}
