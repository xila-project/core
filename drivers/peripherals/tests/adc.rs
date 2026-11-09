#![cfg(all(target_os = "linux", feature = "adc"))]

//! ADC device nodes: a simulated converter is mounted as `/devices/adc/adc0`
//! and driven through the VFS `File` API and control commands.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use file_system::{AccessFlags, Error as FileSystemError, Flags, MemoryDevice};
use peripheral_devices::adc::{
    ABI_VERSION, GET_INFO, InfoAbi, MillivoltsAbi, READ_MILLIVOLTS, SELECT_CHANNEL,
    SelectChannelAbi, VersionAbi, mount_adc,
};
use peripherals::adc::AdcMock;
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

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

fn version() -> VersionAbi {
    VersionAbi {
        version: ABI_VERSION,
        reserved: 0,
    }
}

fn select(channel: u8) -> SelectChannelAbi {
    SelectChannelAbi {
        version: ABI_VERSION,
        channel,
        reserved: 0,
    }
}

fn fs_error(error: FileSystemError) -> Option<Error> {
    Some(Error::FileSystem(error))
}

#[task::test]
async fn plain_read_returns_selected_channel_as_little_endian_u16() {
    let (task, vfs) = initialize().await;

    // Channel 2 holds 0xFFFF, which 12-bit resolution clamps to 0x0FFF.
    let adc = AdcMock::<3>::new(12, Some(3300), [0x0123, 0x0800, 0xFFFF]);
    mount_adc(vfs, task, "adc0", adc).await.unwrap();

    let mut file = open(vfs, task, "/devices/adc/adc0").await.unwrap();

    // Channel 0 is selected by default.
    let mut buffer = [0u8; 4];
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(&buffer[..2], &0x0123u16.to_le_bytes());

    file.control(SELECT_CHANNEL, &select(2)).await.unwrap();
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(&buffer[..2], &[0xFF, 0x0F]);

    // A plain read needs room for one sample.
    assert_eq!(
        file.read(&mut buffer[..1]).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    // Plain writes are not supported on an ADC.
    assert_eq!(
        file.write(b"x").await.err(),
        fs_error(FileSystemError::UnsupportedOperation)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn info_and_millivolts_use_the_converter_reference() {
    let (task, vfs) = initialize().await;

    let adc = AdcMock::<3>::new(12, Some(3300), [0x0800, 0x0000, 0xFFFF]);
    mount_adc(vfs, task, "adc1", adc).await.unwrap();

    let mut file = open(vfs, task, "/devices/adc/adc1").await.unwrap();

    assert_eq!(
        file.control(GET_INFO, &version()).await.unwrap(),
        InfoAbi {
            version: ABI_VERSION,
            channels: 3,
            resolution_bits: 12,
            reference_millivolts: 3300,
            reference_present: 1,
            reserved: [0; 3],
        }
    );

    // 2048 / 4095 of 3300 mV, rounded down.
    assert_eq!(
        file.control(READ_MILLIVOLTS, &version()).await.unwrap(),
        MillivoltsAbi {
            version: ABI_VERSION,
            reserved: 0,
            millivolts: 1650,
        }
    );

    file.control(SELECT_CHANNEL, &select(2)).await.unwrap();
    assert_eq!(
        file.control(READ_MILLIVOLTS, &version())
            .await
            .unwrap()
            .millivolts,
        3300
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn converter_without_reference_reports_it_and_refuses_millivolts() {
    let (task, vfs) = initialize().await;

    let adc = AdcMock::<1>::new(10, None, [512]);
    mount_adc(vfs, task, "adc2", adc).await.unwrap();

    let mut file = open(vfs, task, "/devices/adc/adc2").await.unwrap();

    let info = file.control(GET_INFO, &version()).await.unwrap();
    assert_eq!(info.reference_present, 0);
    assert_eq!(info.reference_millivolts, 0);
    assert_eq!(info.resolution_bits, 10);

    assert_eq!(
        file.control(READ_MILLIVOLTS, &version()).await.err(),
        fs_error(FileSystemError::UnsupportedOperation)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn invalid_channel_and_bad_abi_are_rejected_and_keep_selection() {
    let (task, vfs) = initialize().await;

    let adc = AdcMock::<2>::new(12, Some(3300), [0x0111, 0x0222]);
    mount_adc(vfs, task, "adc3", adc).await.unwrap();

    let mut file = open(vfs, task, "/devices/adc/adc3").await.unwrap();

    assert_eq!(
        file.control(SELECT_CHANNEL, &select(2)).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    let mut buffer = [0u8; 2];
    file.read(&mut buffer).await.unwrap();
    assert_eq!(buffer, 0x0111u16.to_le_bytes(), "selection must not change");

    let wrong_version = SelectChannelAbi {
        version: 2,
        ..select(1)
    };
    assert_eq!(
        file.control(SELECT_CHANNEL, &wrong_version).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    let dirty_reserved = SelectChannelAbi {
        reserved: 1,
        ..select(1)
    };
    assert_eq!(
        file.control(SELECT_CHANNEL, &dirty_reserved).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    assert_eq!(
        file.control(
            GET_INFO,
            &VersionAbi {
                version: 2,
                reserved: 0,
            }
        )
        .await
        .err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn second_open_of_the_same_node_is_busy() {
    let (task, vfs) = initialize().await;

    let adc = AdcMock::<1>::new(12, Some(3300), [0]);
    mount_adc(vfs, task, "adc4", adc).await.unwrap();

    let first = open(vfs, task, "/devices/adc/adc4").await.unwrap();
    assert_eq!(
        open(vfs, task, "/devices/adc/adc4").await.err(),
        fs_error(FileSystemError::RessourceBusy)
    );

    first.close(vfs).await.unwrap();
    open(vfs, task, "/devices/adc/adc4")
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();
}
