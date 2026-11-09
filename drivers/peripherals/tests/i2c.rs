#![cfg(all(target_os = "linux", feature = "i2c"))]

//! I2C device nodes backed by `MockI2cController`, driven through the VFS `File` API
//! and the I2C control commands.

extern crate abi_definitions;
extern crate alloc;

#[path = "support/allocation.rs"]
mod allocation;

use drivers_peripherals::i2c::{
    FrequencyOutput, GET_FREQUENCY, SELECT_ADDRESS, SET_FREQUENCY, SelectAddressInput,
    SetFrequencyInput, WRITE_READ, WriteReadInput, WriteReadOutput, mount_i2c_device,
};
use file_system::{AccessFlags, Error as FileSystemError, Flags, MemoryDevice};
use peripherals::i2c::{
    BusConfig, Capabilities, I2cController, I2cSlave, MockI2cBus, MockI2cController,
};
use task::TaskIdentifier;
use virtual_file_system::{Error, File, VirtualFileSystem};

const CAPABILITIES: Capabilities = Capabilities {
    ten_bit: false,
    max_frequency_hz: 400_000,
};

/// Register-file slave: the first written byte selects the register pointer, and
/// reads return bytes from the pointer onwards.
struct RegisterSlave {
    registers: [u8; 8],
    pointer: usize,
}

impl I2cSlave for RegisterSlave {
    fn write(&mut self, data: &[u8]) -> bool {
        if let Some((&first, rest)) = data.split_first() {
            self.pointer = usize::from(first);
            for byte in rest {
                self.registers[self.pointer % 8] = *byte;
                self.pointer += 1;
            }
        }
        true
    }

    fn read(&mut self, buffer: &mut [u8]) {
        for byte in buffer {
            *byte = self.registers[self.pointer % 8];
            self.pointer += 1;
        }
    }
}

/// Wraps the mock so the device layer can mount it, which requires `PeripheralHandle: Send`.
struct TestController(MockI2cController<'static, 2>);

impl I2cController for TestController {
    type Bus<'a>
        = MockI2cBus<'a, 'static, 2>
    where
        Self: 'a;

    fn capabilities(&self) -> Capabilities {
        self.0.capabilities()
    }

    fn open(&self, config: BusConfig) -> Result<Self::Bus<'_>, peripherals::i2c::Error> {
        self.0.open(config)
    }
}

fn register_slave(registers: [u8; 8]) -> &'static mut dyn I2cSlave {
    Box::leak(Box::new(RegisterSlave {
        registers,
        pointer: 0,
    }))
}

/// Controller with slaves at 0x50 (registers 0x10..0x17) and 0x51 (registers 0x20..0x27).
fn controller() -> TestController {
    let controller = MockI2cController::<2>::new(CAPABILITIES);
    controller
        .attach(
            peripherals::i2c::Address::SevenBit(0x50),
            register_slave([0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]),
        )
        .unwrap();
    controller
        .attach(
            peripherals::i2c::Address::SevenBit(0x51),
            register_slave([0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27]),
        )
        .unwrap();
    TestController(controller)
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

async fn mount_bus(vfs: &VirtualFileSystem, task: TaskIdentifier, name: &str) {
    mount_i2c_device(vfs, task, name, controller(), BusConfig::new(100_000))
        .await
        .unwrap();
}

fn select(ten_bit: u8, address: u16) -> SelectAddressInput {
    SelectAddressInput {
        version: 1,
        ten_bit,
        reserved: 0,
        address,
    }
}

fn transfer(write: &[u8], read_length: u8) -> WriteReadInput {
    let mut input = WriteReadInput {
        write_length: write.len() as u8,
        read_length,
        ..WriteReadInput::default()
    };
    input.write[..write.len()].copy_from_slice(write);
    input
}

#[task::test]
async fn combined_write_then_read_returns_register_data() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus0").await;
    let mut file = open(vfs, task, "/devices/i2c/bus0").await.unwrap();

    file.control(SELECT_ADDRESS, &select(0, 0x50))
        .await
        .unwrap();

    // Write register pointer 3, repeated START, read two bytes in one call.
    let output: WriteReadOutput = file.control(WRITE_READ, &transfer(&[3], 2)).await.unwrap();
    assert_eq!(output.read_length, 2);
    assert_eq!(&output.read[..2], &[0x13, 0x14]);

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn plain_write_then_read_reaches_selected_address() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus1").await;
    let mut file = open(vfs, task, "/devices/i2c/bus1").await.unwrap();

    file.control(SELECT_ADDRESS, &select(0, 0x51))
        .await
        .unwrap();
    assert_eq!(file.write(&[2]).await.unwrap(), 1);
    let mut buffer = [0u8; 2];
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(buffer, [0x22, 0x23]);

    // Selecting the other slave changes where the same calls go.
    file.control(SELECT_ADDRESS, &select(0, 0x50))
        .await
        .unwrap();
    assert_eq!(file.write(&[2]).await.unwrap(), 1);
    assert_eq!(file.read(&mut buffer).await.unwrap(), 2);
    assert_eq!(buffer, [0x12, 0x13]);

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn absent_address_is_nacked_and_reported() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus2").await;
    let mut file = open(vfs, task, "/devices/i2c/bus2").await.unwrap();

    file.control(SELECT_ADDRESS, &select(0, 0x20))
        .await
        .unwrap();
    assert_eq!(
        file.write(&[0]).await.err(),
        Some(Error::FileSystem(FileSystemError::NotFound))
    );
    let mut buffer = [0u8; 1];
    assert_eq!(
        file.read(&mut buffer).await.err(),
        Some(Error::FileSystem(FileSystemError::NotFound))
    );

    file.close(vfs).await.unwrap();
}

#[task::test]
async fn node_is_exclusive_while_open() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus3").await;
    let file = open(vfs, task, "/devices/i2c/bus3").await.unwrap();

    assert_eq!(
        open(vfs, task, "/devices/i2c/bus3").await.err(),
        Some(Error::FileSystem(FileSystemError::RessourceBusy))
    );

    file.close(vfs).await.unwrap();
    open(vfs, task, "/devices/i2c/bus3")
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();
}

#[task::test]
async fn frequency_and_argument_validation() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus4").await;
    let mut file = open(vfs, task, "/devices/i2c/bus4").await.unwrap();

    let frequency: FrequencyOutput = file.control(GET_FREQUENCY, &()).await.unwrap();
    assert_eq!(frequency.frequency_hz, 100_000);
    assert_eq!(frequency.max_frequency_hz, 400_000);
    assert_eq!(frequency.ten_bit_supported, 0);

    file.control(
        SET_FREQUENCY,
        &SetFrequencyInput {
            version: 1,
            reserved: 0,
            frequency_hz: 400_000,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        file.control(
            SET_FREQUENCY,
            &SetFrequencyInput {
                version: 1,
                reserved: 0,
                frequency_hz: 1_000_000,
            },
        )
        .await
        .err(),
        Some(Error::FileSystem(FileSystemError::UnsupportedOperation))
    );
    assert_eq!(
        file.control(
            SET_FREQUENCY,
            &SetFrequencyInput {
                version: 2,
                reserved: 0,
                frequency_hz: 100_000,
            },
        )
        .await
        .err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );

    // Ten-bit addressing is unsupported by this controller.
    assert_eq!(
        file.control(SELECT_ADDRESS, &select(1, 0x123)).await.err(),
        Some(Error::FileSystem(FileSystemError::UnsupportedOperation))
    );
    // Seven-bit addresses above 0x7F are invalid.
    assert_eq!(
        file.control(SELECT_ADDRESS, &select(0, 0x80)).await.err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );
    // Transfers must move at least one byte.
    assert_eq!(
        file.control(WRITE_READ, &transfer(&[], 0)).await.err(),
        Some(Error::FileSystem(FileSystemError::InvalidParameter))
    );
    file.close(vfs).await.unwrap();
}

#[task::test]
async fn plain_io_before_address_selection_fails() {
    let (task, vfs) = initialize().await;
    mount_bus(vfs, task, "bus5").await;
    let mut file = open(vfs, task, "/devices/i2c/bus5").await.unwrap();

    assert_eq!(
        file.write(&[0]).await.err(),
        Some(Error::FileSystem(FileSystemError::NotInitialized))
    );

    file.close(vfs).await.unwrap();
}
