#[cfg(all(test, feature = "mock"))]
mod tests {
    use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};

    use super::{DriveMode, GpioController, GpioMock, InputConfig, LineCapabilities, LineId};
    use super::{OutputConfig, PinState, Pull};

    const CAPABILITIES: LineCapabilities = LineCapabilities {
        input: true,
        output: true,
        pull_up: true,
        pull_down: true,
        open_drain: true,
    };

    fn mock() -> GpioMock<2> {
        GpioMock::new([CAPABILITIES; 2])
    }

    #[test]
    fn invalid_line_is_rejected() {
        let mock = mock();
        let invalid = LineId::new(2);
        assert!(matches!(
            mock.request_input(invalid, InputConfig::default()),
            Err(super::MockError::InvalidLine)
        ));
        assert!(
            mock.request_input(LineId::new(0), InputConfig::default())
                .is_ok()
        );
    }

    #[test]
    fn reserved_line_is_exclusive() {
        let mock = mock();
        let id = LineId::new(0);
        let _input = mock.request_input(id, InputConfig::default()).unwrap();
        assert!(matches!(
            mock.request_input(id, InputConfig::default()),
            Err(super::MockError::AlreadyReserved)
        ));
    }

    #[test]
    fn dropping_line_releases_reservation() {
        let mock = mock();
        let id = LineId::new(0);
        drop(mock.request_input(id, InputConfig::default()).unwrap());
        assert!(mock.request_input(id, InputConfig::default()).is_ok());
    }

    #[test]
    fn output_request_sets_initial_physical_level() {
        let mock = mock();
        let mut output = mock
            .request_output(
                LineId::new(0),
                OutputConfig::new(Pull::Disabled, DriveMode::PushPull, PinState::High),
            )
            .unwrap();
        assert!(output.is_set_high().unwrap());
    }

    #[test]
    fn input_reads_injected_physical_level() {
        let mock = mock();
        let id = LineId::new(0);
        mock.set_input_level(id, PinState::High).unwrap();
        let mut input = mock.request_input(id, InputConfig::default()).unwrap();
        assert!(input.is_high().unwrap());
    }

    #[test]
    fn unsupported_pull_is_rejected() {
        let mock = GpioMock::new([LineCapabilities {
            pull_up: false,
            ..CAPABILITIES
        }]);
        assert!(matches!(
            mock.request_input(LineId::new(0), InputConfig::new(Pull::Up)),
            Err(super::MockError::Unsupported)
        ));
    }

    #[test]
    fn unsupported_drive_mode_is_rejected() {
        let mock = GpioMock::new([LineCapabilities {
            open_drain: false,
            ..CAPABILITIES
        }]);
        assert!(matches!(
            mock.request_output(
                LineId::new(0),
                OutputConfig::new(Pull::Disabled, DriveMode::OpenDrain, PinState::Low)
            ),
            Err(super::MockError::Unsupported)
        ));
    }

    #[test]
    fn output_pin_traits_read_and_write_physical_levels() {
        let mock = mock();
        let mut output = mock
            .request_output(
                LineId::new(0),
                OutputConfig::new(Pull::Disabled, DriveMode::PushPull, PinState::Low),
            )
            .unwrap();
        assert!(!output.is_set_high().unwrap());
        output.set_high().unwrap();
        assert!(output.is_set_high().unwrap());
        output.set_low().unwrap();
        assert!(output.is_set_low().unwrap());
    }

    #[test]
    fn mock_line_handles_fit_in_two_machine_words() {
        assert!(core::mem::size_of::<super::MockInput<'_, 2>>() <= 2 * size_of::<usize>());
        assert!(core::mem::size_of::<super::MockOutput<'_, 2>>() <= 2 * size_of::<usize>());
    }
}
#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::{GpioMock, MockError, MockInput, MockOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineId(u16);

impl LineId {
    pub const fn new(offset: u16) -> Self {
        Self(offset)
    }

    pub const fn offset(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pull {
    Disabled,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinState {
    Low,
    High,
}

impl PinState {
    pub const fn is_high(self) -> bool {
        matches!(self, Self::High)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveMode {
    PushPull,
    OpenDrain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputConfig {
    pub pull: Pull,
}

impl InputConfig {
    pub const fn new(pull: Pull) -> Self {
        Self { pull }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new(Pull::Disabled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputConfig {
    pub pull: Pull,
    pub drive_mode: DriveMode,
    pub initial_state: PinState,
}

impl OutputConfig {
    pub const fn new(pull: Pull, drive_mode: DriveMode, initial_state: PinState) -> Self {
        Self {
            pull,
            drive_mode,
            initial_state,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCapabilities {
    pub input: bool,
    pub output: bool,
    pub pull_up: bool,
    pub pull_down: bool,
    pub open_drain: bool,
}

pub trait GpioController {
    type Error: embedded_hal::digital::Error;
    type Input<'a>: embedded_hal::digital::InputPin<Error = Self::Error>
    where
        Self: 'a;
    type Output<'a>: embedded_hal::digital::OutputPin<Error = Self::Error>
        + embedded_hal::digital::StatefulOutputPin<Error = Self::Error>
    where
        Self: 'a;

    fn capabilities(&self, line: LineId) -> Option<LineCapabilities>;

    fn request_input(
        &self,
        line: LineId,
        config: InputConfig,
    ) -> Result<Self::Input<'_>, Self::Error>;

    fn request_output(
        &self,
        line: LineId,
        config: OutputConfig,
    ) -> Result<Self::Output<'_>, Self::Error>;
}
