use audio::{AudioStreamDevice, AudioSystem, Configuration, START, STOP, SampleFormat};
use drivers_headless::HeadlessAudioOutput;
use file_system::{BaseOperations, Context, ControlCommand, Error};
use shared::AnyByLayout;

type System = AudioSystem<2, 32, 2, 1, 32>;

fn pcm(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

#[test]
fn captures_real_mixed_samples_into_caller_storage() {
    let system = core::pin::pin!(System::new().unwrap());
    let mut output = HeadlessAudioOutput::new(system.as_ref()).unwrap();
    let first = system.open_stream().unwrap();
    let second = system.open_stream().unwrap();
    first.write(&pcm(&[0.25, -0.25, 0.125, -0.125])).unwrap();
    second.write(&pcm(&[0.125, 0.125, 0.25, 0.25])).unwrap();
    first.start().unwrap();
    second.start().unwrap();

    system.as_ref().mix_next_block().unwrap();
    let mut captured = [0u8; 16];
    assert_eq!(output.read(&mut captured), Ok(16));
    assert_eq!(captured.as_slice(), pcm(&[0.375, -0.125, 0.375, 0.125]));
    assert_eq!(
        output.configuration(),
        Configuration::new(48_000, 2, SampleFormat::F32)
    );
    assert_eq!(output.read(&mut captured), Ok(0));
}

#[test]
fn silence_requires_explicit_mixing_and_does_not_overwrite_unused_storage() {
    let system = core::pin::pin!(System::new().unwrap());
    let mut output = HeadlessAudioOutput::new(system.as_ref()).unwrap();
    let mut captured = [0xff; 24];
    assert_eq!(output.read(&mut captured), Ok(0));
    assert_eq!(captured, [0xff; 24]);

    system.as_ref().mix_next_block().unwrap();
    assert_eq!(output.read(&mut captured), Ok(16));
    assert_eq!(&captured[..16], &[0; 16]);
    assert_eq!(&captured[16..], &[0xff; 8]);
}

#[test]
fn reads_whole_frames_and_leaves_queued_samples_intact_on_invalid_buffers() {
    let system = core::pin::pin!(System::new().unwrap());
    let mut output = HeadlessAudioOutput::new(system.as_ref()).unwrap();
    let stream = system.open_stream().unwrap();
    stream.write(&pcm(&[0.25, -0.25, 0.125, -0.125])).unwrap();
    stream.start().unwrap();
    system.as_ref().mix_next_block().unwrap();

    assert_eq!(output.read(&mut [0; 7]), Err(Error::InvalidParameter));
    assert_eq!(output.read(&mut []), Ok(0));
    let mut frame = [0u8; 8];
    assert_eq!(output.read(&mut frame), Ok(8));
    assert_eq!(frame.as_slice(), pcm(&[0.25, -0.25]));
    assert_eq!(output.read(&mut frame), Ok(8));
    assert_eq!(frame.as_slice(), pcm(&[0.125, -0.125]));
    assert_eq!(output.read(&mut frame), Ok(0));
}

#[test]
fn bounded_output_reports_overflow_and_drop_releases_its_reservation() {
    let system = core::pin::pin!(System::new().unwrap());
    let output = HeadlessAudioOutput::new(system.as_ref()).unwrap();
    assert!(matches!(
        HeadlessAudioOutput::new(system.as_ref()),
        Err(Error::RessourceBusy)
    ));
    for _ in 0..3 {
        system.as_ref().mix_next_block().unwrap();
    }
    assert_eq!(output.dropped_blocks(), 1);
    drop(output);

    let mut replacement = HeadlessAudioOutput::new(system.as_ref()).unwrap();
    assert_eq!(replacement.dropped_blocks(), 0);
    assert_eq!(replacement.read(&mut [0; 32]), Ok(0));
}

#[test]
fn stream_device_controls_and_drain_work_without_a_hardware_backend() {
    // The VFS device requires a static service; this small test fixture owns it
    // for the process lifetime, just like the audio library's device tests.
    let system = Box::leak(Box::new(Box::pin(System::new().unwrap()))).as_ref();
    let device = AudioStreamDevice::new(system);
    let mut output = HeadlessAudioOutput::new(system).unwrap();
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();
    let samples = pcm(&[0.25, -0.25, 0.125, -0.125]);
    assert_eq!(device.write_sync(&mut context, &samples, 0), Ok(16));
    assert_eq!(device.flush_sync(&mut context), Err(Error::RessourceBusy));

    let mut captured = [0xff; 16];
    system.mix_next_block().unwrap();
    assert_eq!(output.read(&mut captured), Ok(16));
    assert_eq!(captured, [0; 16]); // Streams start paused.
    device
        .control_sync(
            &mut context,
            START::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut ()),
        )
        .unwrap();
    system.mix_next_block().unwrap();
    assert_eq!(output.read(&mut captured), Ok(16));
    assert_eq!(captured.as_slice(), samples);
    assert_eq!(device.flush_sync(&mut context), Ok(()));

    device
        .control_sync(
            &mut context,
            STOP::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut ()),
        )
        .unwrap();
    system.mix_next_block().unwrap();
    assert_eq!(output.read(&mut captured), Ok(16));
    assert_eq!(captured, [0; 16]);
    device.close(&mut context).unwrap();
}
