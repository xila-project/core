#![cfg(all(target_os = "linux", feature = "human-interface"))]
#![allow(
    clippy::disallowed_types,
    reason = "host-only test uses std::sync::Arc"
)]

//! Human interface emulation node: a recording controller is mounted as
//! `/devices/human_interface/emulation` and driven through the VFS `File` API: writes of
//! 8-byte messages become HID reports, invalid writes fail, reads are unsupported and the
//! `GET_STATUS` control command returns the connection state and keyboard indicators.

extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use std::sync::{Arc, Mutex};

use drivers_peripherals::human_interface::{
    ABI_VERSION, EMULATION_PATH, GET_STATUS, INDICATOR_CAPS_LOCK, MESSAGE_SIZE, Message, Modifiers,
    MouseButtons, StatusAbi, VersionAbi, keyboard, mount_human_interface, system,
};
use file_system::{AccessFlags, Flags, IoErrorKind, MemoryDevice};
use peripherals::human_interface::{
    Error as ControllerError, HumanInterfaceController, REPORT_ID_CONSUMER, REPORT_ID_KEYBOARD,
    REPORT_ID_MOUSE,
};
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

#[derive(Default)]
struct Recorded {
    /// Every report received, as (identifier, bytes).
    reports: Vec<(u8, Vec<u8>)>,
    connected: bool,
    indicators: u8,
    fail_with: Option<ControllerError>,
}

/// `Send` controller with shared state, so a test can inspect what the mounted node sent.
#[derive(Clone, Default)]
struct SharedController {
    state: Arc<Mutex<Recorded>>,
}

impl HumanInterfaceController for SharedController {
    fn send_report(&mut self, identifier: u8, report: &[u8]) -> Result<(), ControllerError> {
        let mut state = self.state.lock().unwrap();
        if let Some(error) = state.fail_with {
            return Err(error);
        }
        state.reports.push((identifier, report.to_vec()));
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.state.lock().unwrap().connected
    }

    fn indicators(&self) -> u8 {
        self.state.lock().unwrap().indicators
    }
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
    let options = little_fs::Options::new(256);
    little_fs::FileSystem::format(device, options).unwrap();
    let file_system = little_fs::FileSystem::new(device, options).unwrap();

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

#[task::test]
async fn mounted_node_forwards_messages_as_reports_and_rejects_invalid_writes() {
    let (task, vfs) = initialize().await;

    let controller = SharedController::default();
    controller.state.lock().unwrap().connected = true;
    controller.state.lock().unwrap().indicators = INDICATOR_CAPS_LOCK;
    mount_human_interface(vfs, task, "emulation", controller.clone())
        .await
        .unwrap();

    let mut file = open(vfs, task, EMULATION_PATH).await.unwrap();

    // A second opening of the same node is refused while the first is held.
    assert_eq!(
        open(vfs, task, EMULATION_PATH).await.err(),
        Some(IoErrorKind::ResourceBusy)
    );

    // One write holds several 8-byte messages; each becomes one report.
    let mut buffer = [0u8; 3 * MESSAGE_SIZE];
    buffer[..MESSAGE_SIZE].copy_from_slice(
        &Message::Keyboard {
            modifiers: Modifiers::LEFT_CONTROL | Modifiers::LEFT_SHIFT,
            keys: [keyboard::A, keyboard::ENTER, 0, 0, 0, 0],
        }
        .encode(),
    );
    buffer[MESSAGE_SIZE..2 * MESSAGE_SIZE].copy_from_slice(
        &Message::Mouse {
            buttons: MouseButtons::LEFT,
            x: -10,
            y: 20,
            wheel: 1,
            pan: 0,
        }
        .encode(),
    );
    buffer[2 * MESSAGE_SIZE..].copy_from_slice(
        &Message::Consumer {
            usage: device::human_interface::consumer::VOLUME_UP,
        }
        .encode(),
    );
    assert_eq!(file.write(&buffer).await.unwrap(), buffer.len());

    {
        let state = controller.state.lock().unwrap();
        let reports = &state.reports;
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].0, REPORT_ID_KEYBOARD);
        assert_eq!(
            reports[0].1,
            vec![0b0000_0011, 0x00, keyboard::A, keyboard::ENTER, 0, 0, 0, 0]
        );
        assert_eq!(reports[1].0, REPORT_ID_MOUSE);
        assert_eq!(reports[1].1, vec![0x01, 0xF6, 0x14, 0x01, 0x00]);
        assert_eq!(reports[2].0, REPORT_ID_CONSUMER);
        assert_eq!(reports[2].1, vec![0xE9, 0x00]);
    }

    // Invalid writes fail with an invalid-input error and send nothing.
    assert_eq!(
        file.write(&[0u8; MESSAGE_SIZE - 1]).await.err(),
        Some(IoErrorKind::InvalidArgument)
    );
    let mut unknown_kind = Message::System { usage: 0 }.encode();
    unknown_kind[0] = 99;
    assert_eq!(
        file.write(&unknown_kind).await.err(),
        Some(IoErrorKind::InvalidArgument)
    );
    // A valid message followed by an invalid one is rejected as a whole.
    let mut mixed = [0u8; 2 * MESSAGE_SIZE];
    mixed[..MESSAGE_SIZE].copy_from_slice(&Message::System { usage: 0 }.encode());
    mixed[MESSAGE_SIZE] = 200;
    assert_eq!(
        file.write(&mixed).await.err(),
        Some(IoErrorKind::InvalidArgument)
    );
    assert_eq!(controller.state.lock().unwrap().reports.len(), 3);

    // Reads are unsupported: the node is write-only for reports.
    let mut read_buffer = [0u8; 8];
    assert_eq!(
        file.read(&mut read_buffer).await.err(),
        Some(IoErrorKind::Unsupported)
    );

    // The status comes from the GET_STATUS control command.
    let status = file.control(GET_STATUS, &version()).await.unwrap();
    assert_eq!(
        status,
        StatusAbi {
            version: ABI_VERSION,
            connected: 1,
            indicators: INDICATOR_CAPS_LOCK,
            reserved: [0; 4],
        }
    );
    assert_eq!(
        file.control(
            GET_STATUS,
            &VersionAbi {
                version: ABI_VERSION + 1,
                reserved: 0
            }
        )
        .await
        .err(),
        Some(IoErrorKind::InvalidArgument)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn controller_failures_map_to_file_system_errors() {
    let (task, vfs) = initialize().await;

    let controller = SharedController::default();
    controller.state.lock().unwrap().fail_with = Some(ControllerError::NotConnected);
    mount_human_interface(vfs, task, "emulation", controller.clone())
        .await
        .unwrap();

    let mut file = open(vfs, task, EMULATION_PATH).await.unwrap();
    assert_eq!(
        file.write(
            &Message::System {
                usage: system::POWER_DOWN
            }
            .encode()
        )
        .await
        .err(),
        Some(IoErrorKind::NotConnected)
    );
    assert!(controller.state.lock().unwrap().reports.is_empty());

    controller.state.lock().unwrap().fail_with = Some(ControllerError::Other);
    assert_eq!(
        file.write(
            &Message::System {
                usage: system::POWER_DOWN
            }
            .encode()
        )
        .await
        .err(),
        Some(IoErrorKind::InputOutput)
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn mounting_twice_or_with_a_bad_name_is_refused() {
    let (task, vfs) = initialize().await;

    mount_human_interface(vfs, task, "emulation", SharedController::default())
        .await
        .unwrap();
    assert!(
        mount_human_interface(vfs, task, "emulation", SharedController::default())
            .await
            .is_err()
    );
    for bad_name in ["", ".", "..", "a/b"] {
        assert_eq!(
            mount_human_interface(vfs, task, bad_name, SharedController::default())
                .await
                .err(),
            Some(IoErrorKind::InvalidPath)
        );
    }
}
