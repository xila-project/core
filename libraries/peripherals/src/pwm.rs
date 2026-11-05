//! Xila PWM contract on top of `embedded-hal`.
//!
//! Drivers use `embedded_hal::pwm::SetDutyCycle` on an opened channel. Xila owns
//! what embedded-hal lacks: naming channels, bounding the period (frequency),
//! reserving a channel for one holder at a time, and the error vocabulary.

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::{MockPwmChannel, PwmMock};

/// Index of a PWM channel on a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelId(u8);

impl ChannelId {
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u8 {
        self.0
    }
}

/// Limits a controller can express. `max_duty_cycle` is fixed for the lifetime
/// of the controller, so duty values keep their meaning across period changes.
/// `min_frequency_hz == max_frequency_hz` describes a fixed-period controller,
/// which rejects `set_frequency` with [`Error::Unsupported`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub channel_count: u8,
    pub min_frequency_hz: u32,
    pub max_frequency_hz: u32,
    pub max_duty_cycle: u16,
}

/// Errors seen by PWM users. embedded-hal 1.0 names only `ErrorKind::Other` for
/// PWM, so every variant reports that kind; the variants keep the Xila meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The channel index does not exist on this controller.
    InvalidChannel,
    /// The frequency is zero or outside the controller's bounds.
    InvalidFrequency,
    /// The duty is above the channel's `max_duty_cycle`.
    InvalidDuty,
    /// The channel is already held by another handle.
    Busy,
    /// The controller cannot perform the operation (for example, a fixed period).
    Unsupported,
    Other,
}

impl embedded_hal::pwm::Error for Error {
    fn kind(&self) -> embedded_hal::pwm::ErrorKind {
        embedded_hal::pwm::ErrorKind::Other
    }
}

/// Controller that owns a set of channels and hands out exclusive channel handles.
///
/// A channel handle reserves its channel until it is dropped. The period is per
/// channel, and `set_frequency` may be called while a handle is open.
pub trait PwmController {
    type Channel<'a>: embedded_hal::pwm::SetDutyCycle<Error = Error>
    where
        Self: 'a;

    fn capabilities(&self) -> Capabilities;

    /// Reserves `channel` until the returned handle is dropped. Fails with
    /// [`Error::InvalidChannel`] or [`Error::Busy`].
    fn open(&self, channel: ChannelId) -> Result<Self::Channel<'_>, Error>;

    /// Sets the period of `channel` in hertz.
    fn set_frequency(&self, channel: ChannelId, frequency_hz: u32) -> Result<(), Error>;

    /// Returns the current period of `channel` in hertz.
    fn frequency_hz(&self, channel: ChannelId) -> Result<u32, Error>;
}
