#![cfg(all(target_os = "linux", feature = "spi"))]

//! SPI device nodes mounted over a bus backed by `peripherals::spi::SpiMock`,
//! driven through the VFS `File` API and the control commands.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use std::sync::atomic::{AtomicBool, Ordering};

use drivers_peripherals::spi::{
    SPI_ABI_VERSION, SPI_GET_CONFIGURATION, SPI_MAX_SEGMENTS, SPI_MAX_TRANSACTION_BYTES,
    SPI_MAX_TRANSFER_BYTES, SPI_SEGMENT_READ, SPI_SEGMENT_TRANSFER, SPI_SEGMENT_WRITE,
    SPI_SET_CONFIGURATION, SPI_TRANSACTION, SPI_TRANSFER, SpiConfigurationAbi, SpiSegmentAbi,
    SpiTransactionInputAbi, SpiTransferInputAbi, mount_spi_device,
};
use file_system::{AccessFlags, Error as FileSystemError, Flags, MemoryDevice, Position};
use peripherals::spi::{
    BusCapabilities, DeviceConfig, Mode, SimulatedSpiDevice, SpiController, SpiError, SpiMock,
    SpiMockReservation,
};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

/// Simulated device: every byte it receives is answered with `byte + selects`,
/// where `selects` counts chip-select assertions so far. Responses therefore
/// reveal how many chip-select cycles happened before each transfer.
#[derive(Default)]
struct Responder {
    selects: u8,
}

impl SimulatedSpiDevice for Responder {
    fn chip_select(&mut self, asserted: bool) {
        if asserted {
            self.selects = self.selects.wrapping_add(1);
        }
    }

    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) {
        for (out, byte) in rx.iter_mut().zip(tx) {
            *out = byte.wrapping_add(self.selects);
        }
    }
}

const CAPABILITIES: BusCapabilities = BusCapabilities {
    max_frequency_hz: 10_000_000,
    modes: [true, false, true, false],
    lsb_first: false,
};

/// A shared bus whose only endpoint is chip select 0, with the responder leaked so the
/// bus is `'static` and `Send`, as mounting requires.
struct SimulatedBus {
    bus: SpiMock<'static, 2>,
    /// Set by the test to model another master holding the bus.
    external_master: AtomicBool,
}

impl SpiController for SimulatedBus {
    type Reservation<'a>
        = SpiMockReservation<'a, 'static, 2>
    where
        Self: 'a;

    fn reserve(&self) -> Result<Self::Reservation<'_>, SpiError> {
        if self.external_master.load(Ordering::Acquire) {
            return Err(SpiError::Busy);
        }
        self.bus.reserve()
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

fn leak_bus() -> &'static SimulatedBus {
    let responder: &'static Mutex<CriticalSectionRawMutex, Responder> =
        Box::leak(Box::new(Mutex::new(Responder::default())));
    Box::leak(Box::new(SimulatedBus {
        bus: SpiMock::new(CAPABILITIES, [Some(responder), None]),
        external_master: AtomicBool::new(false),
    }))
}

fn initial_config() -> DeviceConfig {
    DeviceConfig::new(Mode::Mode0, 1_000_000)
}

fn transfer_input(tx: &[u8]) -> SpiTransferInputAbi {
    let mut input = SpiTransferInputAbi {
        version: SPI_ABI_VERSION,
        length: tx.len() as u16,
        tx: [0; SPI_MAX_TRANSFER_BYTES],
    };
    input.tx[..tx.len()].copy_from_slice(tx);
    input
}

fn transaction_input(segments: &[SpiSegmentAbi], data: &[u8]) -> SpiTransactionInputAbi {
    let mut input = SpiTransactionInputAbi {
        version: SPI_ABI_VERSION,
        segment_count: segments.len() as u8,
        reserved: 0,
        segments: [SpiSegmentAbi::default(); SPI_MAX_SEGMENTS],
        data: [0; SPI_MAX_TRANSACTION_BYTES],
    };
    input.segments[..segments.len()].copy_from_slice(segments);
    input.data[..data.len()].copy_from_slice(data);
    input
}

fn segment(kind: u8, cs_change: u8, length: u16) -> SpiSegmentAbi {
    SpiSegmentAbi {
        kind,
        cs_change,
        length,
    }
}

#[task::test]
async fn spi_node_is_exclusive_and_plain_io_reads_zeros_on_mosi() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();

    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();

    // A second opening while the first is held is refused.
    assert_eq!(
        open(vfs, task, "/devices/spi/spi0.0").await.err(),
        Some(Error::FileSystem(FileSystemError::RessourceBusy))
    );

    // Write: one chip-select cycle, responses are discarded.
    assert_eq!(file.write(&[1, 2]).await.unwrap(), 2);

    // Read: a second cycle, clocking zeros, so each byte is `0 + 2`.
    file.set_position(&Position::Start(0)).await.unwrap();
    let mut buffer = [0u8; 2];
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(buffer, [2, 2]);

    file.close(vfs).await.unwrap();
    open(vfs, task, "/devices/spi/spi0.0")
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();
}

#[task::test]
async fn configuration_is_read_and_validated_through_the_contract() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();

    let initial = file.control(SPI_GET_CONFIGURATION, &()).await.unwrap();
    assert_eq!(initial.version, SPI_ABI_VERSION);
    assert_eq!((initial.mode, initial.bit_order), (0, 0));
    assert_eq!(initial.frequency_hz, 1_000_000);
    assert_eq!(initial.bits_per_word, 8);

    let requested = SpiConfigurationAbi {
        version: SPI_ABI_VERSION,
        mode: 2,
        bit_order: 0,
        frequency_hz: 2_000_000,
        bits_per_word: 8,
        reserved: [0; 3],
    };
    file.control(SPI_SET_CONFIGURATION, &requested)
        .await
        .unwrap();
    let updated = file.control(SPI_GET_CONFIGURATION, &()).await.unwrap();
    assert_eq!((updated.mode, updated.frequency_hz), (2, 2_000_000));

    // Settings the mock does not support are refused and leave the config unchanged.
    let unsupported_mode = SpiConfigurationAbi {
        mode: 1,
        ..requested
    };
    assert_eq!(
        file.control(SPI_SET_CONFIGURATION, &unsupported_mode)
            .await
            .err(),
        Some(Error::FileSystem(FileSystemError::UnsupportedOperation))
    );
    let unsupported_frequency = SpiConfigurationAbi {
        frequency_hz: 20_000_000,
        ..requested
    };
    assert_eq!(
        file.control(SPI_SET_CONFIGURATION, &unsupported_frequency)
            .await
            .err(),
        Some(Error::FileSystem(FileSystemError::UnsupportedOperation))
    );
    let lsb_first = SpiConfigurationAbi {
        bit_order: 1,
        ..requested
    };
    assert_eq!(
        file.control(SPI_SET_CONFIGURATION, &lsb_first).await.err(),
        Some(Error::FileSystem(FileSystemError::UnsupportedOperation))
    );

    // Malformed values are rejected before the contract sees them.
    let bad_mode = SpiConfigurationAbi {
        mode: 9,
        ..requested
    };
    assert_eq!(
        file.control(SPI_SET_CONFIGURATION, &bad_mode).await.err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );
    let bad_version = SpiConfigurationAbi {
        version: SPI_ABI_VERSION + 1,
        ..requested
    };
    assert_eq!(
        file.control(SPI_SET_CONFIGURATION, &bad_version)
            .await
            .err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );

    let after = file.control(SPI_GET_CONFIGURATION, &()).await.unwrap();
    assert_eq!(after, updated);
    assert_eq!(after.bit_order, 0);

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn full_duplex_transfer_returns_the_simulated_response() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();

    // One chip-select cycle, so each received byte is `tx + 1`.
    let output = file
        .control(SPI_TRANSFER, &transfer_input(&[1, 2, 3]))
        .await
        .unwrap();
    assert_eq!(output.version, SPI_ABI_VERSION);
    assert_eq!(output.length, 3);
    assert_eq!(&output.rx[..3], &[2, 3, 4]);
    assert_eq!(&output.rx[3..], &[0; SPI_MAX_TRANSFER_BYTES - 3]);

    let too_long = SpiTransferInputAbi {
        version: SPI_ABI_VERSION,
        length: SPI_MAX_TRANSFER_BYTES as u16 + 1,
        tx: [0; SPI_MAX_TRANSFER_BYTES],
    };
    assert_eq!(
        file.control(SPI_TRANSFER, &too_long).await.err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn multi_segment_transaction_honours_cs_change_in_order() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();

    // Write (cs_change), then full duplex, then read in the same selection.
    // Selects: write asserts once (1), cs_change releases, full duplex asserts
    // again (2) and returns 5 + 2 = 7, then the read reuses the held selection
    // and returns 0 + 2 = 2.
    let segments = [
        segment(SPI_SEGMENT_WRITE, 1, 1),
        segment(SPI_SEGMENT_TRANSFER, 0, 1),
        segment(SPI_SEGMENT_READ, 0, 1),
    ];
    let output = file
        .control(SPI_TRANSACTION, &transaction_input(&segments, &[1, 5, 0]))
        .await
        .unwrap();
    assert_eq!(output.version, SPI_ABI_VERSION);
    assert_eq!(output.length, 3);
    assert_eq!(&output.data[..3], &[0, 7, 2]);

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn transaction_rejects_out_of_bounds_input() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();
    let invalid = Some(Error::FileSystem(FileSystemError::InvalidParameter));

    let one = [segment(SPI_SEGMENT_WRITE, 0, 1)];

    let mut wrong_version = transaction_input(&one, &[1]);
    wrong_version.version = SPI_ABI_VERSION + 1;
    assert_eq!(
        file.control(SPI_TRANSACTION, &wrong_version).await.err(),
        invalid
    );

    let mut no_segments = transaction_input(&one, &[1]);
    no_segments.segment_count = 0;
    assert_eq!(
        file.control(SPI_TRANSACTION, &no_segments).await.err(),
        invalid
    );

    let mut too_many = transaction_input(&one, &[1]);
    too_many.segment_count = SPI_MAX_SEGMENTS as u8 + 1;
    assert_eq!(
        file.control(SPI_TRANSACTION, &too_many).await.err(),
        invalid
    );

    let unknown_kind = [segment(7, 0, 1)];
    assert_eq!(
        file.control(SPI_TRANSACTION, &transaction_input(&unknown_kind, &[1]))
            .await
            .err(),
        invalid
    );

    // Two segments of 33 and 32 bytes total 65, one more than the bound.
    let oversized = [
        segment(SPI_SEGMENT_WRITE, 0, 33),
        segment(SPI_SEGMENT_WRITE, 0, 32),
    ];
    assert_eq!(
        file.control(SPI_TRANSACTION, &transaction_input(&oversized, &[]))
            .await
            .err(),
        invalid
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn invalid_chip_select_surfaces_as_not_found() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    // The simulated bus has no endpoint on chip select 1.
    mount_spi_device(vfs, task, 0, 1, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.1").await.unwrap();

    assert_eq!(
        file.write(&[1]).await.err(),
        Some(Error::FileSystem(FileSystemError::NotFound))
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn bus_held_by_another_master_surfaces_as_busy() {
    let (task, vfs) = initialize().await;
    let bus = leak_bus();

    mount_spi_device(vfs, task, 0, 0, bus, initial_config())
        .await
        .unwrap();
    let mut file = open(vfs, task, "/devices/spi/spi0.0").await.unwrap();

    bus.external_master.store(true, Ordering::Release);
    assert_eq!(
        file.write(&[1]).await.err(),
        Some(Error::FileSystem(FileSystemError::RessourceBusy))
    );
    bus.external_master.store(false, Ordering::Release);

    assert_eq!(file.write(&[1]).await.unwrap(), 1);
    file.close(vfs).await.unwrap();
}
