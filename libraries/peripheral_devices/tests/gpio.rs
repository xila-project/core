#![cfg(all(target_os = "linux", feature = "gpio"))]

use file_system::{ControlCommand, Error};
use peripheral_devices::{
    PeripheralHandle,
    gpio::{
        ABI_VERSION, CONFIGURE, CapabilitiesAbi, ConfigureAbi, GET_CAPABILITIES, GpioHandle,
        RELEASE, VersionAbi,
    },
};
use peripherals::gpio::{GpioMock, LineCapabilities, LineId, PinState};
use shared::AnyByLayout;

const CAPABILITIES: LineCapabilities = LineCapabilities {
    input: true,
    output: true,
    pull_up: true,
    pull_down: true,
    open_drain: true,
};

fn controller(capabilities: LineCapabilities) -> &'static GpioMock<2> {
    Box::leak(Box::new(GpioMock::new([capabilities; 2])))
}

fn configuration(direction: u8, pull: u8, drive_mode: u8, initial_level: u8) -> ConfigureAbi {
    ConfigureAbi {
        version: ABI_VERSION,
        direction,
        pull,
        drive_mode,
        initial_level,
        reserved: 0,
    }
}

const OUTPUT_LOW: (u8, u8, u8, u8) = (1, 0, 0, 0);
const INPUT: (u8, u8, u8, u8) = (0, 0, 0, 0);

fn configure(
    handle: &mut GpioHandle<GpioMock<2>>,
    request: ConfigureAbi,
) -> file_system::Result<()> {
    let mut output = VersionAbi::default();
    handle.control(
        CONFIGURE::IDENTIFIER,
        AnyByLayout::from(&request),
        AnyByLayout::from_mutable(&mut output),
    )
}

fn release(handle: &mut GpioHandle<GpioMock<2>>) -> file_system::Result<()> {
    let input = VersionAbi {
        version: ABI_VERSION,
        reserved: 0,
    };
    let mut output = VersionAbi::default();
    handle.control(
        RELEASE::IDENTIFIER,
        AnyByLayout::from(&input),
        AnyByLayout::from_mutable(&mut output),
    )
}

fn write(handle: &mut GpioHandle<GpioMock<2>>, level: u8) -> file_system::Result<usize> {
    handle.write(&[level])
}

fn read(handle: &mut GpioHandle<GpioMock<2>>) -> file_system::Result<u8> {
    let mut byte = [0u8];
    handle.read(&mut byte)?;
    Ok(byte[0])
}

fn configured(
    controller: &'static GpioMock<2>,
    line: u16,
    request: (u8, u8, u8, u8),
) -> GpioHandle<GpioMock<2>> {
    let mut handle = GpioHandle::new(controller, LineId::new(line));
    configure(
        &mut handle,
        configuration(request.0, request.1, request.2, request.3),
    )
    .unwrap();
    handle
}

#[test]
fn output_writes_are_read_back() {
    let controller = controller(CAPABILITIES);
    let mut handle = configured(controller, 0, OUTPUT_LOW);

    assert_eq!(read(&mut handle), Ok(0));
    assert_eq!(write(&mut handle, 1), Ok(1));
    assert_eq!(read(&mut handle), Ok(1));
    assert_eq!(write(&mut handle, 0), Ok(1));
    assert_eq!(read(&mut handle), Ok(0));
}

#[test]
fn input_reads_the_line_level() {
    let controller = controller(CAPABILITIES);
    controller
        .set_input_level(LineId::new(0), PinState::High)
        .unwrap();
    let mut handle = configured(controller, 0, INPUT);

    assert_eq!(read(&mut handle), Ok(1));
}

#[test]
fn writing_an_input_is_unsupported() {
    let controller = controller(CAPABILITIES);
    let mut handle = configured(controller, 0, INPUT);

    assert_eq!(write(&mut handle, 1), Err(Error::UnsupportedOperation));
}

#[test]
fn unconfigured_line_reports_not_initialized() {
    let controller = controller(CAPABILITIES);
    let mut handle = GpioHandle::new(controller, LineId::new(0));

    assert_eq!(read(&mut handle), Err(Error::NotInitialized));
    assert_eq!(write(&mut handle, 1), Err(Error::NotInitialized));
}

#[test]
fn invalid_fields_are_rejected() {
    let controller = controller(CAPABILITIES);
    let mut handle = GpioHandle::new(controller, LineId::new(0));

    let invalid = [
        configuration(2, 0, 0, 0),
        configuration(0, 3, 0, 0),
        configuration(1, 0, 2, 0),
        configuration(1, 0, 0, 2),
    ];
    for request in invalid {
        assert_eq!(
            configure(&mut handle, request),
            Err(Error::InvalidParameter)
        );
    }

    let wrong_version = ConfigureAbi {
        version: ABI_VERSION + 1,
        ..configuration(0, 0, 0, 0)
    };
    assert_eq!(
        configure(&mut handle, wrong_version),
        Err(Error::InvalidParameter)
    );

    let reserved_set = ConfigureAbi {
        reserved: 1,
        ..configuration(0, 0, 0, 0)
    };
    assert_eq!(
        configure(&mut handle, reserved_set),
        Err(Error::InvalidParameter)
    );
}

#[test]
fn reconfiguring_releases_the_previous_line() {
    let controller = controller(CAPABILITIES);
    let mut handle = configured(controller, 0, OUTPUT_LOW);

    configure(&mut handle, configuration(0, 0, 0, 0)).unwrap();
    assert_eq!(write(&mut handle, 1), Err(Error::UnsupportedOperation));
}

#[test]
fn release_frees_the_line_for_another_handle() {
    let controller = controller(CAPABILITIES);
    let mut first = configured(controller, 1, OUTPUT_LOW);
    let mut second = GpioHandle::new(controller, LineId::new(1));

    assert_eq!(
        configure(&mut second, configuration(0, 0, 0, 0)),
        Err(Error::InputOutput)
    );

    release(&mut first).unwrap();
    configure(&mut second, configuration(0, 0, 0, 0)).unwrap();
}

#[test]
fn unsupported_pull_is_reported_as_input_output_error() {
    let controller = controller(LineCapabilities {
        pull_up: false,
        ..CAPABILITIES
    });
    let mut handle = GpioHandle::new(controller, LineId::new(0));

    assert_eq!(
        configure(&mut handle, configuration(0, 1, 0, 0)),
        Err(Error::InputOutput)
    );
}

#[test]
fn capabilities_are_reported_through_control() {
    let controller = controller(CAPABILITIES);
    let mut handle = GpioHandle::new(controller, LineId::new(0));

    let input = VersionAbi {
        version: ABI_VERSION,
        reserved: 0,
    };
    let mut output = CapabilitiesAbi::default();
    handle
        .control(
            GET_CAPABILITIES::IDENTIFIER,
            AnyByLayout::from(&input),
            AnyByLayout::from_mutable(&mut output),
        )
        .unwrap();

    assert_eq!(
        output,
        CapabilitiesAbi {
            version: ABI_VERSION,
            input: 1,
            output: 1,
            pull_up: 1,
            pull_down: 1,
            open_drain: 1,
            reserved: 0,
        }
    );
}

#[test]
fn unknown_command_is_unsupported() {
    let controller = controller(CAPABILITIES);
    let mut handle = GpioHandle::new(controller, LineId::new(0));

    let input = VersionAbi::default();
    let mut output = VersionAbi::default();
    assert_eq!(
        handle.control(
            file_system::ControlCommandIdentifier::new::<VersionAbi, VersionAbi>(
                file_system::ControlDirectionFlags::Write,
                b'g',
                0x7f,
            ),
            AnyByLayout::from(&input),
            AnyByLayout::from_mutable(&mut output),
        ),
        Err(Error::UnsupportedOperation)
    );
}
