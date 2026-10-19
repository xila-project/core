use audio::{
    AudioStreamDevice, AudioSystem, Configuration, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE,
    SET_CONFIGURATION, SET_VOLUME, START, STOP, SampleFormat,
};
use file_system::{BaseOperations, Context, ControlCommand, Error};
use shared::AnyByLayout;
use std::{boxed::Box, pin::Pin};

type TestSystem = AudioSystem<2, 64, 2, 1, 16>;

fn service() -> (Pin<&'static TestSystem>, &'static TestSystem) {
    let system = Box::leak(Box::new(Box::pin(TestSystem::new().unwrap())));
    let pinned = system.as_ref();
    let reference = pinned.get_ref();
    (pinned, reference)
}

fn control<C: ControlCommand>(
    device: &impl BaseOperations,
    context: &mut Context,
    input: &C::Input,
    output: &mut C::Output,
) -> file_system::Result<()> {
    device.control_sync(
        context,
        C::IDENTIFIER,
        AnyByLayout::from(input),
        AnyByLayout::from_mutable(output),
    )
}

fn open(device: &impl BaseOperations) -> Context {
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();
    context
}

#[test]
fn contexts_own_isolated_streams_and_controls() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut first = open(&device);
    let mut second = open(&device);
    let samples = [0.25f32, -0.25].map(f32::to_le_bytes).concat();

    assert_eq!(device.write_sync(&mut first, &samples, 0), Ok(8));
    control::<SET_VOLUME>(&device, &mut first, &30, &mut ()).unwrap();

    let mut first_writable = 0;
    let mut second_writable = 0;
    control::<GET_WRITABLE>(&device, &mut first, &(), &mut first_writable).unwrap();
    control::<GET_WRITABLE>(&device, &mut second, &(), &mut second_writable).unwrap();
    assert_eq!(first_writable, 56);
    assert_eq!(second_writable, 64);

    let mut first_volume = 0;
    let mut second_volume = 0;
    control::<GET_VOLUME>(&device, &mut first, &(), &mut first_volume).unwrap();
    control::<GET_VOLUME>(&device, &mut second, &(), &mut second_volume).unwrap();
    assert_eq!(first_volume, 30);
    assert_eq!(second_volume, 100);
}

#[test]
fn close_and_reopen_reuses_a_clean_paused_stream() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut context = open(&device);
    device.write_sync(&mut context, &[0; 8], 0).unwrap();
    control::<START>(&device, &mut context, &(), &mut ()).unwrap();
    device.close(&mut context).unwrap();

    device.open(&mut context).unwrap();
    let mut writable = 0;
    let mut volume = 0;
    control::<GET_WRITABLE>(&device, &mut context, &(), &mut writable).unwrap();
    control::<GET_VOLUME>(&device, &mut context, &(), &mut volume).unwrap();
    assert_eq!(writable, 64);
    assert_eq!(volume, 100);
    assert_eq!(device.flush_sync(&mut context), Ok(()));
}

#[test]
fn failed_open_does_not_reserve_a_stream_slot() {
    type OneStream = AudioSystem<1, 64, 2, 1, 16>;
    let system = Box::leak(Box::new(Box::pin(OneStream::new().unwrap())));
    let device = AudioStreamDevice::new(system.as_ref());
    let mut first = open(&device);
    let mut second = Context::new_empty();

    assert_eq!(device.open(&mut second), Err(Error::RessourceBusy));
    device.close(&mut first).unwrap();
    assert_eq!(device.open(&mut second), Ok(()));
    device.close(&mut second).unwrap();
}

#[test]
fn unopened_context_rejects_operations_and_read_is_unsupported() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut context = Context::new_empty();
    let mut output = [0; 8];

    assert_eq!(
        device.write_sync(&mut context, &[0; 8], 0),
        Err(Error::InvalidContext)
    );
    assert_eq!(device.flush_sync(&mut context), Err(Error::InvalidContext));
    assert_eq!(
        device.control_sync(
            &mut context,
            GET_VOLUME::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut 0u8),
        ),
        Err(Error::InvalidContext)
    );
    assert_eq!(
        device.read_sync(&mut context, &mut output, 0),
        Err(Error::UnsupportedOperation)
    );
}

#[test]
fn controls_validate_payloads_values_and_fixed_configuration() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut context = open(&device);
    let fixed = Configuration::new(48_000, 2, SampleFormat::F32);
    let other = Configuration::new(44_100, 2, SampleFormat::F32);
    let mut actual = Configuration::new(1, 1, SampleFormat::U8);

    control::<GET_CONFIGURATION>(&device, &mut context, &(), &mut actual).unwrap();
    assert_eq!(actual, fixed);
    control::<SET_CONFIGURATION>(&device, &mut context, &fixed, &mut ()).unwrap();
    assert_eq!(
        control::<SET_CONFIGURATION>(&device, &mut context, &other, &mut ()),
        Err(Error::UnsupportedOperation)
    );
    assert_eq!(
        control::<SET_VOLUME>(&device, &mut context, &101, &mut ()),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        device.write_sync(&mut context, &[0; 7], 0),
        Err(Error::InvalidParameter)
    );

    let wrong_input = 0u32;
    let mut empty = ();
    assert_eq!(
        device.control_sync(
            &mut context,
            SET_CONFIGURATION::IDENTIFIER,
            AnyByLayout::from(&wrong_input),
            AnyByLayout::from_mutable(&mut empty),
        ),
        Err(Error::InvalidParameter)
    );

    let mut wrong_output = 0u16;
    assert_eq!(
        device.control_sync(
            &mut context,
            GET_VOLUME::IDENTIFIER,
            AnyByLayout::NONE,
            AnyByLayout::from_mutable(&mut wrong_output),
        ),
        Err(Error::InvalidParameter)
    );

    let mut misaligned = [0u8; size_of::<Configuration>() + 1];
    let unaligned_configuration = unsafe {
        AnyByLayout::from_raw_parts(misaligned.as_mut_ptr().add(1), size_of::<Configuration>())
    };
    assert_eq!(
        device.control_sync(
            &mut context,
            SET_CONFIGURATION::IDENTIFIER,
            unaligned_configuration,
            AnyByLayout::from_mutable(&mut empty),
        ),
        Err(Error::InvalidParameter)
    );
}

#[test]
fn full_stream_writes_return_only_complete_frame_progress() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut context = open(&device);
    let samples = [0.25f32; 18].map(f32::to_le_bytes).concat();

    assert_eq!(device.write_sync(&mut context, &samples, 0), Ok(64));
    assert_eq!(device.write_sync(&mut context, &samples[..8], 64), Ok(0));
}

#[test]
fn controls_start_stop_and_write_contribute_expected_mix() {
    let (system, _) = service();
    let device = AudioStreamDevice::new(system);
    let mut context = open(&device);
    let mut output = system.open_output().unwrap();
    let samples = [0.5f32, -0.5].map(f32::to_le_bytes).concat();

    device.write_sync(&mut context, &samples, 0).unwrap();
    control::<START>(&device, &mut context, &(), &mut ()).unwrap();
    assert_eq!(device.flush_sync(&mut context), Err(Error::RessourceBusy));
    system.as_ref().mix_next_block().unwrap();
    let mut rendered = [0.0f32; 4];
    output.with_consumer(|consumer| {
        let bytes = consumer.readable();
        let (chunks, remainder) = bytes.as_chunks::<4>();
        assert!(remainder.is_empty());
        for (sample, chunk) in rendered.iter_mut().zip(chunks) {
            *sample = f32::from_le_bytes(*chunk);
        }
    });
    assert_eq!(rendered, [0.5, -0.5, 0.0, 0.0]);

    control::<STOP>(&device, &mut context, &(), &mut ()).unwrap();
    assert_eq!(device.flush_sync(&mut context), Ok(()));
    assert!(matches!(
        device.clone_context(&context),
        Err(Error::UnsupportedOperation)
    ));
}
