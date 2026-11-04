#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::{AtResponder, LoopbackEndpoint, LoopbackUart};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowControl {
    None,
    RtsCts,
    XonXoff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub stop_bits: StopBits,
    pub parity: Parity,
    pub flow_control: FlowControl,
}

impl Config {
    pub const fn new(
        baud_rate: u32,
        data_bits: DataBits,
        stop_bits: StopBits,
        parity: Parity,
        flow_control: FlowControl,
    ) -> Self {
        Self {
            baud_rate,
            data_bits,
            stop_bits,
            parity,
            flow_control,
        }
    }
}

impl Default for Config {
    /// 115200 baud, 8 data bits, 1 stop bit, no parity, no flow control.
    fn default() -> Self {
        Self::new(
            115_200,
            DataBits::Eight,
            StopBits::One,
            Parity::None,
            FlowControl::None,
        )
    }
}

/// Settings a UART driver can express. `StopBits::One`, `Parity::None` and
/// `FlowControl::None` are always supported; the flags below cover the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub min_baud_rate: u32,
    pub max_baud_rate: u32,
    pub data_five: bool,
    pub data_six: bool,
    pub data_seven: bool,
    pub data_eight: bool,
    pub stop_two: bool,
    pub parity_even: bool,
    pub parity_odd: bool,
    pub rts_cts: bool,
    pub xon_xoff: bool,
}

impl Capabilities {
    /// Returns the first setting in `config` that this UART cannot express.
    pub const fn check(&self, config: &Config) -> Result<(), Error> {
        if config.baud_rate < self.min_baud_rate || config.baud_rate > self.max_baud_rate {
            return Err(Error::UnsupportedBaudRate);
        }
        let data_supported = match config.data_bits {
            DataBits::Five => self.data_five,
            DataBits::Six => self.data_six,
            DataBits::Seven => self.data_seven,
            DataBits::Eight => self.data_eight,
        };
        if !data_supported {
            return Err(Error::UnsupportedDataBits);
        }
        let stop_supported = match config.stop_bits {
            StopBits::One => true,
            StopBits::Two => self.stop_two,
        };
        if !stop_supported {
            return Err(Error::UnsupportedStopBits);
        }
        let parity_supported = match config.parity {
            Parity::None => true,
            Parity::Even => self.parity_even,
            Parity::Odd => self.parity_odd,
        };
        if !parity_supported {
            return Err(Error::UnsupportedParity);
        }
        let flow_supported = match config.flow_control {
            FlowControl::None => true,
            FlowControl::RtsCts => self.rts_cts,
            FlowControl::XonXoff => self.xon_xoff,
        };
        if !flow_supported {
            return Err(Error::UnsupportedFlowControl);
        }
        Ok(())
    }
}

/// Errors are separated so that line-level faults (framing, parity, overrun)
/// can be told apart from device or transport failures (`Io`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedBaudRate,
    UnsupportedDataBits,
    UnsupportedStopBits,
    UnsupportedParity,
    UnsupportedFlowControl,
    /// No data to read or no room to write, in non-blocking mode.
    WouldBlock,
    Framing,
    ParityError,
    Overrun,
    Io,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedBaudRate => "unsupported baud rate",
            Self::UnsupportedDataBits => "unsupported data bits",
            Self::UnsupportedStopBits => "unsupported stop bits",
            Self::UnsupportedParity => "unsupported parity",
            Self::UnsupportedFlowControl => "unsupported flow control",
            Self::WouldBlock => "operation would block",
            Self::Framing => "framing error",
            Self::ParityError => "parity error",
            Self::Overrun => "receive overrun",
            Self::Io => "I/O error",
        })
    }
}

impl core::error::Error for Error {}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        use embedded_io::ErrorKind;
        match self {
            Self::UnsupportedBaudRate
            | Self::UnsupportedDataBits
            | Self::UnsupportedStopBits
            | Self::UnsupportedParity
            | Self::UnsupportedFlowControl => ErrorKind::Unsupported,
            Self::Framing | Self::ParityError => ErrorKind::InvalidData,
            Self::WouldBlock | Self::Overrun | Self::Io => ErrorKind::Other,
        }
    }
}

/// Xila-owned control surface for a UART endpoint. Data transfer, readiness
/// and draining use the `embedded-io` and `embedded-io-async` traits, which
/// every endpoint must implement; this trait only adds what those traits lack:
/// configuration, the non-blocking flag and queue flushes. Blocking behaviour
/// is the driver's responsibility: a blocking endpoint waits for data or
/// space, a non-blocking one returns `Error::WouldBlock` instead.
pub trait UartControl: embedded_io::ErrorType<Error = Error> {
    fn capabilities(&self) -> Capabilities;

    /// Returns the last successfully configured settings.
    fn config(&self) -> Config;

    /// Rejects settings outside `capabilities()` and leaves the current
    /// configuration unchanged on error.
    fn configure(&mut self, config: Config) -> Result<(), Error>;

    fn is_nonblocking(&self) -> bool;

    fn set_nonblocking(&mut self, nonblocking: bool) -> Result<(), Error>;

    /// Discards buffered received data and pending receive errors.
    fn flush_rx(&mut self) -> Result<(), Error>;

    /// Discards output queued but not yet transmitted. Waiting for transmission
    /// is `embedded_io::Write::flush`.
    fn flush_tx(&mut self) -> Result<(), Error>;
}
