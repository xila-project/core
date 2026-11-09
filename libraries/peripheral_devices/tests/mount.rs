#![cfg(target_os = "linux")]

//! Proof: a test-only loopback byte device is mounted as `/devices/uart/test0`
//! and driven only through the VFS `File` API.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use alloc::vec::Vec;

use file_system::{
    AccessFlags, ControlCommand, ControlCommandIdentifier, Error as FileSystemError, Flags,
    MemoryDevice, Position,
};
use peripheral_devices::{PeripheralHandle, mount_peripheral};
use shared::AnyByLayout;
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

file_system::define_command!(BYTES_QUEUED, Read, device::kind::UART, 0, (), usize);

/// Test-only FIFO: whatever is written is read back in order.
#[derive(Default)]
struct Loopback {
    queue: Vec<u8>,
}

impl PeripheralHandle for Loopback {
    fn read(&mut self, buffer: &mut [u8]) -> file_system::Result<usize> {
        let count = buffer.len().min(self.queue.len());
        buffer[..count].copy_from_slice(&self.queue[..count]);
        self.queue.drain(..count);
        Ok(count)
    }

    fn write(&mut self, buffer: &[u8]) -> file_system::Result<usize> {
        self.queue.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        _input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> file_system::Result<()> {
        if command != BYTES_QUEUED::IDENTIFIER {
            return Err(FileSystemError::UnsupportedOperation);
        }
        let output = BYTES_QUEUED::cast_output(output)?;
        *output = self.queue.len();
        Ok(())
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

#[task::test]
async fn mounted_loopback_is_exclusive_and_round_trips_bytes() {
    let (task, vfs) = initialize().await;
    let path = "/devices/uart/test0";

    vfs.create_directory(task, &file_system::Path::DEVICES)
        .await
        .unwrap();
    vfs.create_directory(task, &file_system::Path::UART_DEVICES)
        .await
        .unwrap();

    mount_peripheral(vfs, task, path, Loopback::default())
        .await
        .unwrap();

    let mut file = open(vfs, task, path).await.unwrap();

    // A second opening while the first is held is refused.
    assert_eq!(
        open(vfs, task, path).await.err(),
        Some(Error::FileSystem(FileSystemError::RessourceBusy))
    );

    assert_eq!(file.write(b"ping").await.unwrap(), 4);
    assert_eq!(file.control(BYTES_QUEUED, &()).await.unwrap(), 4);

    file.set_position(&Position::Start(0)).await.unwrap();
    let mut buffer = [0u8; 4];
    assert_eq!(file.read(&mut buffer).await.unwrap(), 4);
    assert_eq!(&buffer, b"ping");

    file.close(vfs).await.unwrap();

    // Closing releases the device, so it can be opened again.
    let mut reopened = open(vfs, task, path).await.unwrap();
    assert_eq!(reopened.write(b"pong").await.unwrap(), 4);
    reopened.close(vfs).await.unwrap();
}
