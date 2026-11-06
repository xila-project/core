//! Character device exposing one GPIO line of a `GpioController` through the VFS.
//!
//! Reads and writes move one byte, `0` or `1`, carrying the line level.
//! Direction, pull, drive mode and release are typed control commands.

use core::{cell::RefCell, task::Poll};

use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice, Error,
    MountOperations, Result, Size, define_command, ready,
};
use peripherals::gpio::{
    DriveMode, GpioController, InputConfig, LineCapabilities, LineId, OutputConfig, PinState, Pull,
};
use shared::AnyByLayout;
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// Command kind shared by all GPIO line devices.
const KIND: u8 = b'g';

// Configures the line as an input or an output and takes it over.
// Any handle previously held by the device is released first.
define_command!(GPIO_CONFIGURE, Write, KIND, 0, GpioConfiguration, ());

// Releases the line, returning it to high impedance.
define_command!(GPIO_RELEASE, Write, KIND, 1, (), ());

// Reads the capabilities of the line.
define_command!(GPIO_CAPABILITIES, Read, KIND, 2, (), GpioCapabilities);

/// Plain-data layout of `GPIO_CONFIGURE`. Fields are bytes, not enums, so any
/// client bytes can be cast safely; the device validates every value.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioConfiguration {
    /// `0` input, `1` output.
    pub direction: u8,
    /// `0` none, `1` pull up, `2` pull down.
    pub pull: u8,
    /// `0` push-pull, `1` open-drain. Outputs only.
    pub drive_mode: u8,
    /// `0` low, `1` high. Outputs only.
    pub initial_level: u8,
}

/// Plain-data layout of `GPIO_CAPABILITIES`; each field is `0` or `1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpioCapabilities {
    pub input: u8,
    pub output: u8,
    pub pull_up: u8,
    pub pull_down: u8,
    pub open_drain: u8,
}

impl From<LineCapabilities> for GpioCapabilities {
    fn from(capabilities: LineCapabilities) -> Self {
        Self {
            input: u8::from(capabilities.input),
            output: u8::from(capabilities.output),
            pull_up: u8::from(capabilities.pull_up),
            pull_down: u8::from(capabilities.pull_down),
            open_drain: u8::from(capabilities.open_drain),
        }
    }
}

enum Handle<C: GpioController + 'static> {
    Input(C::Input<'static>),
    Output(C::Output<'static>),
}

/// One GPIO line exposed as a character device.
pub struct GpioDevice<C: GpioController + Sync + 'static> {
    controller: &'static C,
    line: LineId,
    handle: Mutex<CriticalSectionRawMutex, RefCell<Option<Handle<C>>>>,
}

impl<C: GpioController + Sync + 'static> GpioDevice<C> {
    pub const fn new(controller: &'static C, line: LineId) -> Self {
        Self {
            controller,
            line,
            handle: Mutex::new(RefCell::new(None)),
        }
    }

    fn configure(&self, configuration: GpioConfiguration) -> Result<()> {
        self.handle.lock(|handle| {
            let mut handle = handle.borrow_mut();

            // Release the previous handle before requesting the line again, or
            // the controller would refuse the new request as already reserved.
            // A failed request leaves the line unconfigured.
            *handle = None;
            *handle = Some(self.request(configuration)?);

            Ok(())
        })
    }

    fn request(&self, configuration: GpioConfiguration) -> Result<Handle<C>> {
        let pull = match configuration.pull {
            0 => Pull::Disabled,
            1 => Pull::Up,
            2 => Pull::Down,
            _ => return Err(Error::InvalidParameter),
        };

        match configuration.direction {
            0 => self
                .controller
                .request_input(self.line, InputConfig::new(pull))
                .map(Handle::Input)
                .map_err(|_| Error::InputOutput),
            1 => {
                let drive_mode = match configuration.drive_mode {
                    0 => DriveMode::PushPull,
                    1 => DriveMode::OpenDrain,
                    _ => return Err(Error::InvalidParameter),
                };
                let initial_state = match configuration.initial_level {
                    0 => PinState::Low,
                    1 => PinState::High,
                    _ => return Err(Error::InvalidParameter),
                };

                self.controller
                    .request_output(
                        self.line,
                        OutputConfig::new(pull, drive_mode, initial_state),
                    )
                    .map(Handle::Output)
                    .map_err(|_| Error::InputOutput)
            }
            _ => Err(Error::InvalidParameter),
        }
    }
}

impl<C: GpioController + Sync + 'static> DirectBaseOperations for GpioDevice<C>
where
    C::Input<'static>: Send,
    C::Output<'static>: Send,
{
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        ready(|| {
            let first = buffer.first_mut().ok_or(Error::InvalidParameter)?;

            self.handle
                .lock(|handle| match handle.borrow_mut().as_mut() {
                    Some(Handle::Input(pin)) => {
                        *first = u8::from(pin.is_high().map_err(|_| Error::InputOutput)?);
                        Ok(1)
                    }
                    Some(Handle::Output(pin)) => {
                        *first = u8::from(pin.is_set_high().map_err(|_| Error::InputOutput)?);
                        Ok(1)
                    }
                    None => Err(Error::NotInitialized),
                })
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _absolute_position: Size,
    ) -> Poll<Result<usize>> {
        ready(|| {
            let level = match buffer.first().ok_or(Error::InvalidParameter)? {
                0 => false,
                1 => true,
                _ => return Err(Error::InvalidParameter),
            };

            self.handle
                .lock(|handle| match handle.borrow_mut().as_mut() {
                    Some(Handle::Output(pin)) => {
                        let result = if level { pin.set_high() } else { pin.set_low() };
                        result.map_err(|_| Error::InputOutput)?;
                        Ok(1)
                    }
                    Some(Handle::Input(_)) => Err(Error::UnsupportedOperation),
                    None => Err(Error::NotInitialized),
                })
        })
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        ready(|| match command {
            GPIO_CONFIGURE::IDENTIFIER => {
                let configuration = *GPIO_CONFIGURE::cast_input(input)?;
                self.configure(configuration)
            }
            GPIO_RELEASE::IDENTIFIER => {
                self.handle.lock(|handle| *handle.borrow_mut() = None);
                Ok(())
            }
            GPIO_CAPABILITIES::IDENTIFIER => {
                let capabilities = self
                    .controller
                    .capabilities(self.line)
                    .ok_or(Error::NotFound)?;
                *GPIO_CAPABILITIES::cast_output(output)? = capabilities.into();
                Ok(())
            }
            _ => Err(Error::UnsupportedOperation),
        })
    }
}

impl<C: GpioController + Sync + 'static> MountOperations for GpioDevice<C>
where
    C::Input<'static>: Send,
    C::Output<'static>: Send,
{
}

impl<C: GpioController + Sync + 'static> DirectCharacterDevice for GpioDevice<C>
where
    C::Input<'static>: Send,
    C::Output<'static>: Send,
{
}

#[cfg(test)]
mod tests {
    use core::{
        sync::atomic::{AtomicBool, Ordering},
        task::{Context, Poll, Waker},
    };

    use alloc::boxed::Box;

    use embedded_hal::digital::ErrorKind;
    use file_system::{ControlCommand, DirectBaseOperations, Error};
    use shared::AnyByLayout;

    use super::*;

    /// Thread-safe controller with two lines, so the device under test can be
    /// `Sync` like the real controllers. Each line tracks reservation and level.
    struct FakeController {
        capabilities: LineCapabilities,
        reserved: [AtomicBool; 2],
        level: [AtomicBool; 2],
    }

    impl FakeController {
        fn new(capabilities: LineCapabilities) -> &'static Self {
            Box::leak(Box::new(Self {
                capabilities,
                reserved: [AtomicBool::new(false), AtomicBool::new(false)],
                level: [AtomicBool::new(false), AtomicBool::new(false)],
            }))
        }

        fn reserve(&self, line: LineId) -> core::result::Result<usize, FakeError> {
            let index = usize::from(line.offset());
            let reserved = self.reserved.get(index).ok_or(FakeError::InvalidLine)?;
            reserved
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map(|_| index)
                .map_err(|_| FakeError::AlreadyReserved)
        }
    }

    #[derive(Debug)]
    enum FakeError {
        InvalidLine,
        AlreadyReserved,
        Unsupported,
    }

    impl embedded_hal::digital::Error for FakeError {
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    struct FakeInput<'a> {
        controller: &'a FakeController,
        index: usize,
    }

    struct FakeOutput<'a> {
        controller: &'a FakeController,
        index: usize,
    }

    impl Drop for FakeInput<'_> {
        fn drop(&mut self) {
            self.controller.reserved[self.index].store(false, Ordering::Release);
        }
    }

    impl Drop for FakeOutput<'_> {
        fn drop(&mut self) {
            self.controller.reserved[self.index].store(false, Ordering::Release);
        }
    }

    impl embedded_hal::digital::ErrorType for FakeInput<'_> {
        type Error = FakeError;
    }

    impl embedded_hal::digital::ErrorType for FakeOutput<'_> {
        type Error = FakeError;
    }

    impl embedded_hal::digital::InputPin for FakeInput<'_> {
        fn is_high(&mut self) -> core::result::Result<bool, FakeError> {
            Ok(self.controller.level[self.index].load(Ordering::Acquire))
        }

        fn is_low(&mut self) -> core::result::Result<bool, FakeError> {
            Ok(!self.is_high()?)
        }
    }

    impl embedded_hal::digital::OutputPin for FakeOutput<'_> {
        fn set_low(&mut self) -> core::result::Result<(), FakeError> {
            self.controller.level[self.index].store(false, Ordering::Release);
            Ok(())
        }

        fn set_high(&mut self) -> core::result::Result<(), FakeError> {
            self.controller.level[self.index].store(true, Ordering::Release);
            Ok(())
        }
    }

    impl embedded_hal::digital::StatefulOutputPin for FakeOutput<'_> {
        fn is_set_high(&mut self) -> core::result::Result<bool, FakeError> {
            Ok(self.controller.level[self.index].load(Ordering::Acquire))
        }

        fn is_set_low(&mut self) -> core::result::Result<bool, FakeError> {
            Ok(!self.is_set_high()?)
        }
    }

    impl GpioController for FakeController {
        type Error = FakeError;
        type Input<'a> = FakeInput<'a>;
        type Output<'a> = FakeOutput<'a>;

        fn capabilities(&self, line: LineId) -> Option<LineCapabilities> {
            (usize::from(line.offset()) < self.reserved.len()).then_some(self.capabilities)
        }

        fn request_input(
            &self,
            line: LineId,
            config: InputConfig,
        ) -> core::result::Result<FakeInput<'_>, FakeError> {
            if !self.capabilities.input
                || config.pull != Pull::Disabled && !self.capabilities.pull_up
            {
                return Err(FakeError::Unsupported);
            }
            let index = self.reserve(line)?;
            Ok(FakeInput {
                controller: self,
                index,
            })
        }

        fn request_output(
            &self,
            line: LineId,
            config: OutputConfig,
        ) -> core::result::Result<FakeOutput<'_>, FakeError> {
            if !self.capabilities.output
                || (config.drive_mode == DriveMode::OpenDrain && !self.capabilities.open_drain)
            {
                return Err(FakeError::Unsupported);
            }
            let index = self.reserve(line)?;
            self.level[index].store(config.initial_state.is_high(), Ordering::Release);
            Ok(FakeOutput {
                controller: self,
                index,
            })
        }
    }

    const CAPABILITIES: LineCapabilities = LineCapabilities {
        input: true,
        output: true,
        pull_up: true,
        pull_down: true,
        open_drain: true,
    };

    fn poll<T>(poll: Poll<file_system::Result<T>>) -> file_system::Result<T> {
        match poll {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("GPIO operations are synchronous"),
        }
    }

    fn configure<C: GpioController + Sync + 'static>(
        device: &GpioDevice<C>,
        configuration: GpioConfiguration,
    ) -> file_system::Result<()>
    where
        C::Input<'static>: Send,
        C::Output<'static>: Send,
    {
        let mut cx = Context::from_waker(Waker::noop());
        poll(DirectBaseOperations::control(
            device,
            &mut cx,
            GPIO_CONFIGURE::IDENTIFIER,
            AnyByLayout::from(&configuration),
            AnyByLayout::from_mutable(&mut ()),
        ))
    }

    fn read<C: GpioController + Sync + 'static>(device: &GpioDevice<C>) -> file_system::Result<u8>
    where
        C::Input<'static>: Send,
        C::Output<'static>: Send,
    {
        let mut cx = Context::from_waker(Waker::noop());
        let mut byte = [0u8];
        poll(DirectBaseOperations::read(device, &mut cx, &mut byte, 0))?;
        Ok(byte[0])
    }

    fn write<C: GpioController + Sync + 'static>(
        device: &GpioDevice<C>,
        level: u8,
    ) -> file_system::Result<usize>
    where
        C::Input<'static>: Send,
        C::Output<'static>: Send,
    {
        let mut cx = Context::from_waker(Waker::noop());
        poll(DirectBaseOperations::write(device, &mut cx, &[level], 0))
    }

    const OUTPUT_LOW: GpioConfiguration = GpioConfiguration {
        direction: 1,
        pull: 0,
        drive_mode: 0,
        initial_level: 0,
    };

    const INPUT: GpioConfiguration = GpioConfiguration {
        direction: 0,
        pull: 0,
        drive_mode: 0,
        initial_level: 0,
    };

    #[test]
    fn output_writes_are_read_back() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        configure(&device, OUTPUT_LOW).unwrap();
        assert_eq!(read(&device), Ok(0));

        assert_eq!(write(&device, 1), Ok(1));
        assert_eq!(read(&device), Ok(1));
        assert_eq!(write(&device, 0), Ok(1));
        assert_eq!(read(&device), Ok(0));
    }

    #[test]
    fn input_reads_the_line_level() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        configure(&device, INPUT).unwrap();
        assert_eq!(read(&device), Ok(0));

        controller.level[0].store(true, Ordering::Release);
        assert_eq!(read(&device), Ok(1));
    }

    #[test]
    fn writing_an_input_is_unsupported() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        configure(&device, INPUT).unwrap();
        assert_eq!(write(&device, 1), Err(Error::UnsupportedOperation));
    }

    #[test]
    fn unconfigured_line_reports_not_initialized() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        assert_eq!(read(&device), Err(Error::NotInitialized));
        assert_eq!(write(&device, 1), Err(Error::NotInitialized));
    }

    #[test]
    fn invalid_configuration_values_are_rejected() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        let bad_direction = GpioConfiguration {
            direction: 2,
            ..INPUT
        };
        let bad_pull = GpioConfiguration { pull: 3, ..INPUT };
        let bad_drive = GpioConfiguration {
            drive_mode: 2,
            ..OUTPUT_LOW
        };
        let bad_level = GpioConfiguration {
            initial_level: 2,
            ..OUTPUT_LOW
        };

        assert_eq!(
            configure(&device, bad_direction),
            Err(Error::InvalidParameter)
        );
        assert_eq!(configure(&device, bad_pull), Err(Error::InvalidParameter));
        assert_eq!(configure(&device, bad_drive), Err(Error::InvalidParameter));
        assert_eq!(configure(&device, bad_level), Err(Error::InvalidParameter));
    }

    #[test]
    fn reconfiguring_releases_the_previous_handle() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        configure(&device, OUTPUT_LOW).unwrap();
        configure(&device, INPUT).unwrap();
        assert_eq!(write(&device, 1), Err(Error::UnsupportedOperation));
    }

    #[test]
    fn release_frees_the_line_for_another_device() {
        let controller = FakeController::new(CAPABILITIES);
        let first = GpioDevice::new(controller, LineId::new(1));
        let second = GpioDevice::new(controller, LineId::new(1));

        configure(&first, OUTPUT_LOW).unwrap();
        assert_eq!(configure(&second, INPUT), Err(Error::InputOutput));

        let mut cx = Context::from_waker(Waker::noop());
        poll(DirectBaseOperations::control(
            &first,
            &mut cx,
            GPIO_RELEASE::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut ()),
        ))
        .unwrap();

        configure(&second, INPUT).unwrap();
    }

    #[test]
    fn unsupported_pull_is_reported_as_input_output_error() {
        let controller = FakeController::new(LineCapabilities {
            pull_up: false,
            ..CAPABILITIES
        });
        let device = GpioDevice::new(controller, LineId::new(0));

        let pull_up = GpioConfiguration { pull: 1, ..INPUT };
        assert_eq!(configure(&device, pull_up), Err(Error::InputOutput));
    }

    #[test]
    fn capabilities_are_reported_through_control() {
        let controller = FakeController::new(CAPABILITIES);
        let device = GpioDevice::new(controller, LineId::new(0));

        let mut cx = Context::from_waker(Waker::noop());
        let mut capabilities = GpioCapabilities {
            input: 0,
            output: 0,
            pull_up: 0,
            pull_down: 0,
            open_drain: 0,
        };
        poll(DirectBaseOperations::control(
            &device,
            &mut cx,
            GPIO_CAPABILITIES::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut capabilities),
        ))
        .unwrap();

        assert_eq!(
            capabilities,
            GpioCapabilities {
                input: 1,
                output: 1,
                pull_up: 1,
                pull_down: 1,
                open_drain: 1,
            }
        );
    }
}
