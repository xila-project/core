use core::cell::RefCell;

use embedded_hal::digital::{ErrorKind, ErrorType, InputPin, OutputPin, StatefulOutputPin};

use super::{DriveMode, GpioController, InputConfig, LineCapabilities, LineId, OutputConfig};
use super::{PinState, Pull};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockError {
    InvalidLine,
    AlreadyReserved,
    Unsupported,
}

impl embedded_hal::digital::Error for MockError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Clone, Copy)]
struct MockLineState {
    reserved: bool,
    output: bool,
    level: PinState,
}

impl MockLineState {
    const fn new() -> Self {
        Self {
            reserved: false,
            output: false,
            level: PinState::Low,
        }
    }
}

pub struct GpioMock<const N: usize> {
    capabilities: [LineCapabilities; N],
    lines: RefCell<[MockLineState; N]>,
}

impl<const N: usize> GpioMock<N> {
    pub const fn new(capabilities: [LineCapabilities; N]) -> Self {
        Self {
            capabilities,
            lines: RefCell::new([const { MockLineState::new() }; N]),
        }
    }

    pub fn set_input_level(&self, line: LineId, level: PinState) -> Result<(), MockError> {
        let index = self.index(line)?;
        if !self.capabilities[index].input {
            return Err(MockError::Unsupported);
        }
        let mut lines = self.lines.borrow_mut();
        if lines[index].reserved && lines[index].output {
            return Err(MockError::Unsupported);
        }
        lines[index].level = level;
        Ok(())
    }

    fn index(&self, line: LineId) -> Result<usize, MockError> {
        let index = usize::from(line.offset());
        if index < N {
            Ok(index)
        } else {
            Err(MockError::InvalidLine)
        }
    }

    fn validate_pull(&self, capabilities: LineCapabilities, pull: Pull) -> Result<(), MockError> {
        match pull {
            Pull::Disabled => Ok(()),
            Pull::Up if capabilities.pull_up => Ok(()),
            Pull::Down if capabilities.pull_down => Ok(()),
            Pull::Up | Pull::Down => Err(MockError::Unsupported),
        }
    }

    fn release(&self, line: LineId) {
        if let Ok(index) = self.index(line) {
            self.lines.borrow_mut()[index].reserved = false;
        }
    }
}

impl<const N: usize> GpioController for GpioMock<N> {
    type Error = MockError;
    type Input<'a>
        = MockInput<'a, N>
    where
        Self: 'a;
    type Output<'a>
        = MockOutput<'a, N>
    where
        Self: 'a;

    fn capabilities(&self, line: LineId) -> Option<LineCapabilities> {
        self.capabilities.get(usize::from(line.offset())).copied()
    }

    fn request_input(
        &self,
        line: LineId,
        config: InputConfig,
    ) -> Result<Self::Input<'_>, Self::Error> {
        let index = self.index(line)?;
        let capabilities = self.capabilities[index];
        if !capabilities.input {
            return Err(MockError::Unsupported);
        }
        self.validate_pull(capabilities, config.pull)?;
        let mut lines = self.lines.borrow_mut();
        let state = &mut lines[index];
        if state.reserved {
            return Err(MockError::AlreadyReserved);
        }
        state.reserved = true;
        state.output = false;
        Ok(MockInput {
            controller: self,
            line,
        })
    }

    fn request_output(
        &self,
        line: LineId,
        config: OutputConfig,
    ) -> Result<Self::Output<'_>, Self::Error> {
        let index = self.index(line)?;
        let capabilities = self.capabilities[index];
        if !capabilities.output {
            return Err(MockError::Unsupported);
        }
        self.validate_pull(capabilities, config.pull)?;
        if config.drive_mode == DriveMode::OpenDrain && !capabilities.open_drain {
            return Err(MockError::Unsupported);
        }
        let mut lines = self.lines.borrow_mut();
        let state = &mut lines[index];
        if state.reserved {
            return Err(MockError::AlreadyReserved);
        }
        state.level = config.initial_state;
        state.reserved = true;
        state.output = true;
        Ok(MockOutput {
            controller: self,
            line,
        })
    }
}

pub struct MockInput<'a, const N: usize> {
    controller: &'a GpioMock<N>,
    line: LineId,
}

impl<const N: usize> ErrorType for MockInput<'_, N> {
    type Error = MockError;
}

impl<const N: usize> InputPin for MockInput<'_, N> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        let index = self.controller.index(self.line)?;
        Ok(self.controller.lines.borrow()[index].level.is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        self.is_high().map(|high| !high)
    }
}

impl<const N: usize> Drop for MockInput<'_, N> {
    fn drop(&mut self) {
        self.controller.release(self.line);
    }
}

pub struct MockOutput<'a, const N: usize> {
    controller: &'a GpioMock<N>,
    line: LineId,
}

impl<const N: usize> ErrorType for MockOutput<'_, N> {
    type Error = MockError;
}

impl<const N: usize> OutputPin for MockOutput<'_, N> {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        let index = self.controller.index(self.line)?;
        self.controller.lines.borrow_mut()[index].level = PinState::Low;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        let index = self.controller.index(self.line)?;
        self.controller.lines.borrow_mut()[index].level = PinState::High;
        Ok(())
    }
}

impl<const N: usize> StatefulOutputPin for MockOutput<'_, N> {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        let index = self.controller.index(self.line)?;
        Ok(self.controller.lines.borrow()[index].level.is_high())
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        self.is_set_high().map(|high| !high)
    }
}

impl<const N: usize> Drop for MockOutput<'_, N> {
    fn drop(&mut self) {
        self.controller.release(self.line);
    }
}
