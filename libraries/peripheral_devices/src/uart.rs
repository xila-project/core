//! UART device nodes under `/devices/uart/<name>`.
//!
//! [`UartHandle`] adapts a `peripherals::uart::UartControl` endpoint to
//! [`PeripheralHandle`]. Applications use plain `File` read, write, poll and
//! close. Configuration and queue flushes go through the control commands
//! below, which are the only UART-specific ABI.
//!
//! # Control commands (kind `b'u'`)
//!
//! | Number | Command      | Direction | Input        | Output       | Effect                              |
//! |--------|--------------|-----------|--------------|--------------|-------------------------------------|
//! | `0x00` | `GET_CONFIG` | Read      | `VersionAbi` | `ConfigAbi`  | Returns the current configuration.  |
//! | `0x01` | `SET_CONFIG` | Write     | `ConfigAbi`  | `VersionAbi` | Applies a configuration atomically. |
//! | `0x02` | `FLUSH_RX`   | Write     | `VersionAbi` | `VersionAbi` | Discards received, unread data.     |
//! | `0x03` | `FLUSH_TX`   | Write     | `VersionAbi` | `VersionAbi` | Discards queued, untransmitted data.|
//!
//! Every struct starts with a `version` field that must equal [`ABI_VERSION`];
//! any other value is `InvalidParameter`. Enum fields use the numeric codes
//! below; an unknown code is `InvalidParameter`, and a setting the driver
//! cannot express (per `Capabilities::check`) is `UnsupportedOperation`.
//!
//! # Flush semantics
//!
//! `PeripheralHandle::flush` is a no-op. `embedded_io::Write::flush` waits for
//! transmission, which the handle must not do, and `UartControl::flush_tx`
//! discards queued output, which an application's `flush` must not do. The
//! discarding behaviour is exposed explicitly as `FLUSH_TX`.

use file_system::{ControlCommand, ControlCommandIdentifier, Error, Result};
use peripherals::uart::{
    Config, DataBits, Error as UartError, FlowControl, Parity, StopBits, UartControl,
};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::uart::{
    ABI_VERSION, ConfigAbi, FLOW_CONTROL_NONE, FLOW_CONTROL_RTS_CTS, FLOW_CONTROL_XON_XOFF,
    FLUSH_RX, FLUSH_TX, GET_CONFIG, PARITY_EVEN, PARITY_NONE, PARITY_ODD, SET_CONFIG, VersionAbi,
};

/// Adapts a UART endpoint to [`PeripheralHandle`].
///
/// The endpoint is switched to non-blocking mode at construction, so each
/// read or write is one attempt: an empty receive or full transmit queue
/// returns `RessourceBusy`.
pub struct UartHandle<U> {
    uart: U,
}

impl<U> UartHandle<U>
where
    U: UartControl + embedded_io::Read + embedded_io::Write,
{
    /// Fails if the endpoint cannot be switched to non-blocking mode.
    pub fn new(mut uart: U) -> Result<Self> {
        uart.set_nonblocking(true).map_err(map_error)?;
        Ok(Self { uart })
    }
}

impl<U> PeripheralHandle for UartHandle<U>
where
    U: UartControl + embedded_io::Read + embedded_io::Write + Send,
{
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        embedded_io::Read::read(&mut self.uart, buffer).map_err(map_error)
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        embedded_io::Write::write(&mut self.uart, buffer).map_err(map_error)
    }

    /// No-op, see the module documentation.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == GET_CONFIG::IDENTIFIER {
            let request = GET_CONFIG::cast_input(input)?;
            check_version(request.version)?;
            *GET_CONFIG::cast_output(output)? = config_to_abi(self.uart.config());
        } else if command == SET_CONFIG::IDENTIFIER {
            let request = SET_CONFIG::cast_input(input)?;
            check_version(request.version)?;
            let config = config_from_abi(request)?;
            // `configure` validates against the capabilities and leaves the
            // current configuration unchanged on error.
            self.uart.configure(config).map_err(map_error)?;
            *SET_CONFIG::cast_output(output)? = version_abi();
        } else if command == FLUSH_RX::IDENTIFIER {
            check_version(FLUSH_RX::cast_input(input)?.version)?;
            self.uart.flush_rx().map_err(map_error)?;
            *FLUSH_RX::cast_output(output)? = version_abi();
        } else if command == FLUSH_TX::IDENTIFIER {
            check_version(FLUSH_TX::cast_input(input)?.version)?;
            self.uart.flush_tx().map_err(map_error)?;
            *FLUSH_TX::cast_output(output)? = version_abi();
        } else {
            return Err(Error::UnsupportedOperation);
        }
        Ok(())
    }
}

fn version_abi() -> VersionAbi {
    VersionAbi {
        version: ABI_VERSION,
    }
}

fn check_version(version: u32) -> Result<()> {
    if version == ABI_VERSION {
        Ok(())
    } else {
        Err(Error::InvalidParameter)
    }
}

fn config_to_abi(config: Config) -> ConfigAbi {
    ConfigAbi {
        version: ABI_VERSION,
        baud_rate: config.baud_rate,
        data_bits: match config.data_bits {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        },
        stop_bits: match config.stop_bits {
            StopBits::One => 1,
            StopBits::Two => 2,
        },
        parity: match config.parity {
            Parity::None => PARITY_NONE,
            Parity::Even => PARITY_EVEN,
            Parity::Odd => PARITY_ODD,
        },
        flow_control: match config.flow_control {
            FlowControl::None => FLOW_CONTROL_NONE,
            FlowControl::RtsCts => FLOW_CONTROL_RTS_CTS,
            FlowControl::XonXoff => FLOW_CONTROL_XON_XOFF,
        },
    }
}

fn config_from_abi(abi: &ConfigAbi) -> Result<Config> {
    let data_bits = match abi.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        8 => DataBits::Eight,
        _ => return Err(Error::InvalidParameter),
    };
    let stop_bits = match abi.stop_bits {
        1 => StopBits::One,
        2 => StopBits::Two,
        _ => return Err(Error::InvalidParameter),
    };
    let parity = match abi.parity {
        PARITY_NONE => Parity::None,
        PARITY_EVEN => Parity::Even,
        PARITY_ODD => Parity::Odd,
        _ => return Err(Error::InvalidParameter),
    };
    let flow_control = match abi.flow_control {
        FLOW_CONTROL_NONE => FlowControl::None,
        FLOW_CONTROL_RTS_CTS => FlowControl::RtsCts,
        FLOW_CONTROL_XON_XOFF => FlowControl::XonXoff,
        _ => return Err(Error::InvalidParameter),
    };
    Ok(Config::new(
        abi.baud_rate,
        data_bits,
        stop_bits,
        parity,
        flow_control,
    ))
}

/// Maps a UART error to the file system error seen by applications.
fn map_error(error: UartError) -> Error {
    match error {
        UartError::WouldBlock => Error::RessourceBusy,
        UartError::UnsupportedBaudRate
        | UartError::UnsupportedDataBits
        | UartError::UnsupportedStopBits
        | UartError::UnsupportedParity
        | UartError::UnsupportedFlowControl => Error::UnsupportedOperation,
        UartError::Framing | UartError::ParityError | UartError::Overrun | UartError::Io => {
            Error::InputOutput
        }
    }
}

/// Mounts `uart` at `/devices/uart/<name>`, creating `/devices` and
/// `/devices/uart` when they do not exist. `name` must be a single path
/// component such as `loop0`.
#[cfg(feature = "vfs")]
pub async fn mount_uart<U>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    name: &str,
    uart: U,
) -> virtual_file_system::Result<()>
where
    U: UartControl + embedded_io::Read + embedded_io::Write + Send + 'static,
{
    use file_system::Path;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(virtual_file_system::Error::FileSystem(Error::InvalidPath));
    }
    let path = Path::UART_DEVICES
        .join(name)
        .ok_or(virtual_file_system::Error::FileSystem(Error::InvalidPath))?;
    let handle = UartHandle::new(uart).map_err(virtual_file_system::Error::FileSystem)?;

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::UART_DEVICES)
            .await,
    )?;

    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}
