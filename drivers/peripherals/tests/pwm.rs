#![cfg(all(target_os = "linux", feature = "pwm"))]

//! PWM device nodes: a `PwmMock` controller is mounted as `/devices/pwm/pwm0`
//! and driven through the VFS `File` API, plain writes and control commands.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use file_system::{AccessFlags, Error as FileSystemError, Flags, MemoryDevice};
use peripheral_devices::pwm::{
    GET_INFO, PwmDutyPermilleAbi, PwmFrequencyAbi, PwmInfoAbi, PwmVersionAbi, SET_DUTY_PERMILLE,
    SET_FREQUENCY, mount_pwm,
};
use peripherals::pwm::{ChannelId, PwmController, PwmMock};
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

const MIN_FREQUENCY_HZ: u32 = 1_000;
const MAX_FREQUENCY_HZ: u32 = 100_000;
const MAX_DUTY: u16 = 1_200;

fn leaked_mock() -> &'static PwmMock<2> {
    Box::leak(Box::new(PwmMock::new(
        MIN_FREQUENCY_HZ,
        MAX_FREQUENCY_HZ,
        MAX_DUTY,
    )))
}

async fn initialize() -> (TaskIdentifier, &'static VirtualFileSystem) {
    let time_manager = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let task_instance = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));

    if !log::is_initialized() {
        log::initialize(&testing::Logger).unwrap();
    }

    let task = task_instance.get_current_task_identifier().await;

    let device = MemoryDevice::<512>::new_static(1024 * 512);
    let cache_size = 256;
    little_fs::FileSystem::format(device, cache_size).unwrap();
    let file_system = little_fs::FileSystem::new(device, cache_size).unwrap();

    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(task_instance, users_manager, time_manager, file_system)
            .unwrap(),
    ));

    (task, virtual_file_system)
}

async fn open<'a>(
    vfs: &'a VirtualFileSystem,
    task: TaskIdentifier,
    path: &str,
) -> virtual_file_system::Result<File<'a>> {
    File::open(
        vfs,
        task,
        path,
        Flags::new(AccessFlags::READ_WRITE, None, None),
    )
    .await
}

fn version() -> PwmVersionAbi {
    PwmVersionAbi {
        version: 1,
        reserved: 0,
    }
}

fn fs_error(error: FileSystemError) -> Option<Error> {
    Some(Error::FileSystem(error))
}

#[task::test]
async fn plain_write_and_read_round_trip_the_duty() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    mount_pwm(vfs, task, "pwm0", mock, ChannelId::new(0))
        .await
        .unwrap();

    let mut file = open(vfs, task, "/devices/pwm/pwm0").await.unwrap();

    assert_eq!(file.write(&600u16.to_le_bytes()).await.unwrap(), 2);
    assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(600));

    let mut buffer = [0u8; 2];
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(u16::from_le_bytes(buffer), 600);

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn out_of_range_and_partial_writes_are_rejected() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    mount_pwm(vfs, task, "range0", mock, ChannelId::new(0))
        .await
        .unwrap();

    let mut file = open(vfs, task, "/devices/pwm/range0").await.unwrap();

    assert_eq!(
        file.write(&(MAX_DUTY + 1).to_le_bytes()).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    assert_eq!(
        file.write(&[0x01]).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    assert_eq!(
        file.write(&[0x01, 0x00, 0x00]).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    // Nothing was applied by the rejected writes.
    assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(0));

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn permille_maps_to_the_channel_max_duty() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    mount_pwm(vfs, task, "permille0", mock, ChannelId::new(0))
        .await
        .unwrap();

    let mut file = open(vfs, task, "/devices/pwm/permille0").await.unwrap();

    let info: PwmInfoAbi = file
        .control(
            SET_DUTY_PERMILLE,
            &PwmDutyPermilleAbi {
                version: 1,
                permille: 500,
            },
        )
        .await
        .unwrap();
    assert_eq!(info.max_duty_cycle, MAX_DUTY);
    assert_eq!(mock.duty_cycle(ChannelId::new(0)), Ok(MAX_DUTY / 2));

    assert_eq!(
        file.control(
            SET_DUTY_PERMILLE,
            &PwmDutyPermilleAbi {
                version: 1,
                permille: 1_001,
            },
        )
        .await
        .err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn frequency_is_set_and_reported_by_get_info() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    mount_pwm(vfs, task, "freq0", mock, ChannelId::new(1))
        .await
        .unwrap();

    let mut file = open(vfs, task, "/devices/pwm/freq0").await.unwrap();

    let initial = file.control(GET_INFO, &version()).await.unwrap();
    assert_eq!(initial.channel_count, 2);
    assert_eq!(initial.max_duty_cycle, MAX_DUTY);
    assert_eq!(initial.frequency_hz, MIN_FREQUENCY_HZ);

    let info = file
        .control(
            SET_FREQUENCY,
            &PwmFrequencyAbi {
                version: 1,
                reserved: 0,
                frequency_hz: 20_000,
            },
        )
        .await
        .unwrap();
    assert_eq!(info.frequency_hz, 20_000);
    assert_eq!(mock.frequency_hz(ChannelId::new(1)), Ok(20_000));
    assert_eq!(
        file.control(GET_INFO, &version())
            .await
            .unwrap()
            .frequency_hz,
        20_000
    );

    assert_eq!(
        file.control(
            SET_FREQUENCY,
            &PwmFrequencyAbi {
                version: 1,
                reserved: 0,
                frequency_hz: MAX_FREQUENCY_HZ + 1,
            },
        )
        .await
        .err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    // Unknown ABI versions and non-zero reserved fields are refused.
    assert_eq!(
        file.control(
            GET_INFO,
            &PwmVersionAbi {
                version: 2,
                reserved: 0
            }
        )
        .await
        .err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    assert_eq!(
        file.control(
            GET_INFO,
            &PwmVersionAbi {
                version: 1,
                reserved: 7
            }
        )
        .await
        .err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn second_open_of_the_node_is_busy() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    mount_pwm(vfs, task, "busy0", mock, ChannelId::new(0))
        .await
        .unwrap();

    let file = open(vfs, task, "/devices/pwm/busy0").await.unwrap();
    assert_eq!(
        open(vfs, task, "/devices/pwm/busy0").await.err(),
        fs_error(FileSystemError::RessourceBusy)
    );

    file.close(vfs).await.unwrap();
    open(vfs, task, "/devices/pwm/busy0").await.unwrap();
}

#[task::test]
async fn channel_held_by_another_user_fails_the_mount() {
    let (task, vfs) = initialize().await;
    let mock = leaked_mock();
    let _held = mock.open(ChannelId::new(0)).unwrap();

    assert!(
        mount_pwm(vfs, task, "held0", mock, ChannelId::new(0))
            .await
            .is_err()
    );
}
