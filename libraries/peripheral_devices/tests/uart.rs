#![cfg(all(target_os = "linux", feature = "uart"))]

//! UART device nodes: a loopback pair is mounted as `/devices/uart/loop0` and
//! `/devices/uart/loop1` and driven through the VFS `File` API and control
//! commands.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use file_system::{AccessFlags, Error as FileSystemError, Flags, MemoryDevice};
use peripheral_devices::uart::{
    ConfigAbi, FLOW_CONTROL_RTS_CTS, FLOW_CONTROL_XON_XOFF, FLUSH_RX, FLUSH_TX, GET_CONFIG,
    PARITY_EVEN, SET_CONFIG, VersionAbi, mount_uart,
};
use peripherals::uart::{Capabilities, Config, Error as UartError, UartControl};
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

fn capabilities() -> Capabilities {
    Capabilities {
        min_baud_rate: 9_600,
        max_baud_rate: 921_600,
        data_five: true,
        data_six: true,
        data_seven: true,
        data_eight: true,
        stop_two: true,
        parity_even: true,
        parity_odd: true,
        rts_cts: true,
        xon_xoff: false,
    }
}

#[derive(Default)]
struct Wire {
    /// `queues[side]` holds bytes received by endpoint `side`.
    queues: [VecDeque<u8>; 2],
}

/// One end of a test loopback. Unlike `peripherals::uart::LoopbackUart`, it is
/// `Send` (shared state sits behind `Arc<Mutex<_>>`), which `PeripheralHandle`
/// requires. Empty reads report `WouldBlock`, as a non-blocking endpoint does.
struct LoopbackEnd {
    wire: Arc<Mutex<Wire>>,
    side: usize,
    config: Config,
    capabilities: Capabilities,
    nonblocking: bool,
}

fn loopback_pair() -> (LoopbackEnd, LoopbackEnd) {
    let wire = Arc::new(Mutex::new(Wire::default()));
    let end = |side| LoopbackEnd {
        wire: wire.clone(),
        side,
        config: Config::default(),
        capabilities: capabilities(),
        nonblocking: false,
    };
    (end(0), end(1))
}

impl embedded_io::ErrorType for LoopbackEnd {
    type Error = UartError;
}

impl embedded_io::Read for LoopbackEnd {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, UartError> {
        let mut wire = self.wire.lock().unwrap();
        let queue = &mut wire.queues[self.side];
        if buffer.is_empty() {
            return Ok(0);
        }
        if queue.is_empty() {
            return Err(UartError::WouldBlock);
        }
        let count = buffer.len().min(queue.len());
        for slot in &mut buffer[..count] {
            *slot = queue.pop_front().unwrap();
        }
        Ok(count)
    }
}

impl embedded_io::Write for LoopbackEnd {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, UartError> {
        let mut wire = self.wire.lock().unwrap();
        wire.queues[1 - self.side].extend(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), UartError> {
        Ok(())
    }
}

impl UartControl for LoopbackEnd {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn config(&self) -> Config {
        self.config
    }

    fn configure(&mut self, config: Config) -> Result<(), UartError> {
        self.capabilities.check(&config)?;
        self.config = config;
        Ok(())
    }

    fn is_nonblocking(&self) -> bool {
        self.nonblocking
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<(), UartError> {
        self.nonblocking = nonblocking;
        Ok(())
    }

    fn flush_rx(&mut self) -> Result<(), UartError> {
        self.wire.lock().unwrap().queues[self.side].clear();
        Ok(())
    }

    fn flush_tx(&mut self) -> Result<(), UartError> {
        // Writes are delivered at once, so nothing is queued on the sender side.
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

fn version() -> VersionAbi {
    VersionAbi { version: 1 }
}

fn fs_error(error: FileSystemError) -> Option<Error> {
    Some(Error::FileSystem(error))
}

#[task::test]
async fn loopback_ends_are_mounted_and_exchange_bytes_through_plain_file_io() {
    let (task, vfs) = initialize().await;

    let (end0, end1) = loopback_pair();

    mount_uart(vfs, task, "loop0", end0).await.unwrap();
    mount_uart(vfs, task, "loop1", end1).await.unwrap();

    let mut sender = open(vfs, task, "/devices/uart/loop0").await.unwrap();
    let mut receiver = open(vfs, task, "/devices/uart/loop1").await.unwrap();

    // A second opening of the same node is refused while the first is held.
    assert_eq!(
        open(vfs, task, "/devices/uart/loop0").await.err(),
        fs_error(FileSystemError::RessourceBusy)
    );

    assert_eq!(sender.write(b"ping").await.unwrap(), 4);
    sender.flush().await.unwrap();

    let mut buffer = [0u8; 8];
    let count = receiver.read(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..count], b"ping");

    // Nothing is left, so an empty read is a non-blocking busy result.
    assert_eq!(
        receiver.read(&mut buffer).await.err(),
        fs_error(FileSystemError::RessourceBusy)
    );

    // Reverse direction.
    assert_eq!(receiver.write(b"pong").await.unwrap(), 4);
    let count = sender.read(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..count], b"pong");

    sender.close(vfs).await.unwrap();
    receiver.close(vfs).await.unwrap();

    // Mounting the same name twice is refused.
    let (spare_end, _) = loopback_pair();
    assert!(mount_uart(vfs, task, "loop0", spare_end).await.is_err());
}

#[task::test]
async fn configuration_round_trips_through_control_and_rejects_bad_values() {
    let (task, vfs) = initialize().await;

    let (end0, _end1) = loopback_pair();
    mount_uart(vfs, task, "cfg0", end0).await.unwrap();

    let mut file = open(vfs, task, "/devices/uart/cfg0").await.unwrap();

    let default = file.control(GET_CONFIG, &version()).await.unwrap();
    assert_eq!(default.baud_rate, 115_200);
    assert_eq!(default.data_bits, 8);
    assert_eq!(default.stop_bits, 1);
    assert_eq!(default.parity, 0);
    assert_eq!(default.flow_control, 0);

    let requested = ConfigAbi {
        version: 1,
        baud_rate: 57_600,
        data_bits: 7,
        stop_bits: 2,
        parity: PARITY_EVEN,
        flow_control: FLOW_CONTROL_RTS_CTS,
    };
    assert_eq!(
        file.control(SET_CONFIG, &requested).await.unwrap(),
        version()
    );
    assert_eq!(
        file.control(GET_CONFIG, &version()).await.unwrap(),
        requested
    );

    // A setting the driver cannot express leaves the configuration unchanged.
    let unsupported = ConfigAbi {
        flow_control: FLOW_CONTROL_XON_XOFF,
        ..requested
    };
    assert_eq!(
        file.control(SET_CONFIG, &unsupported).await.err(),
        fs_error(FileSystemError::UnsupportedOperation)
    );
    assert_eq!(
        file.control(GET_CONFIG, &version()).await.unwrap(),
        requested
    );

    // Unknown enum codes and unknown ABI versions are invalid parameters.
    let bad_parity = ConfigAbi {
        parity: 9,
        ..requested
    };
    assert_eq!(
        file.control(SET_CONFIG, &bad_parity).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    let bad_version = ConfigAbi {
        version: 2,
        ..requested
    };
    assert_eq!(
        file.control(SET_CONFIG, &bad_version).await.err(),
        fs_error(FileSystemError::InvalidParameter)
    );
    assert_eq!(
        file.control(GET_CONFIG, &VersionAbi { version: 2 })
            .await
            .err(),
        fs_error(FileSystemError::InvalidParameter)
    );

    // Queue flushes are accepted with a matching version.
    assert_eq!(file.control(FLUSH_RX, &version()).await.unwrap(), version());
    assert_eq!(file.control(FLUSH_TX, &version()).await.unwrap(), version());

    file.close(vfs).await.unwrap();
}
