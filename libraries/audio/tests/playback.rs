use audio::{
    Configuration, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE, PlaybackBuffer, SET_CONFIGURATION,
    SET_VOLUME, START, STOP, SampleFormat,
};
use file_system::{ControlCommand, DirectBaseOperations, Error};
use shared::AnyByLayout;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct CountingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_allocation() {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn control<C: ControlCommand>(
    device: &impl DirectBaseOperations,
    input: &C::Input,
    output: &mut C::Output,
) -> file_system::Result<()> {
    device.control_sync(
        C::IDENTIFIER,
        AnyByLayout::from(input),
        AnyByLayout::from_mutable(output),
    )
}

fn start(device: &impl DirectBaseOperations) {
    control::<START>(device, &(), &mut ()).unwrap();
}

#[test]
fn configuration_and_frame_boundaries() {
    for configuration in [
        Configuration::new(0, 2, SampleFormat::I16),
        Configuration::new(48_000, 0, SampleFormat::I16),
        Configuration::new(48_000, 9, SampleFormat::I16),
    ] {
        assert_eq!(configuration.validate(), Err(Error::InvalidParameter));
    }
    let configuration = Configuration::new(48_000, 2, SampleFormat::I16);
    assert_eq!(configuration.frame_size(), Ok(4));
    assert_eq!(size_of::<Configuration>(), 8);
    assert_eq!(SampleFormat::try_from(255), Err(Error::InvalidParameter));
    assert!(PlaybackBuffer::<6>::new().split(configuration).is_err());
    assert!(PlaybackBuffer::<2>::new().split(configuration).is_err());

    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, _renderer) = buffer.split(configuration).unwrap();
    assert_eq!(
        device.write_sync(&[1, 2, 3], 0),
        Err(Error::InvalidParameter)
    );
    assert_eq!(device.write_sync(&[0; 12], 0), Ok(8));
    assert_eq!(device.write_sync(&[0; 4], 0), Ok(0));
    let mut writable = 99;
    control::<GET_WRITABLE>(&device, &(), &mut writable).unwrap();
    assert_eq!(writable, 0);
}

#[test]
fn starts_paused_and_retains_audio_across_stop() {
    let configuration = Configuration::new(48_000, 1, SampleFormat::I16);
    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, mut renderer) = buffer.split(configuration).unwrap();
    device.write_sync(&1000i16.to_le_bytes(), 0).unwrap();
    let mut output = [42i16; 2];
    assert_eq!(renderer.render_i16(&mut output), 0);
    assert_eq!(output, [0, 0]);
    assert_eq!(device.flush_sync(), Err(Error::RessourceBusy));
    start(&device);
    control::<STOP>(&device, &(), &mut ()).unwrap();
    assert_eq!(renderer.render_i16(&mut output), 0);
    start(&device);
    assert_eq!(renderer.render_i16(&mut output), 1);
    assert_eq!(output, [1000, 0]);
    assert_eq!(device.flush_sync(), Ok(()));
}

#[test]
fn wrapping_stereo_frames_are_not_split_or_reordered() {
    let mut buffer = PlaybackBuffer::<12>::new();
    let (device, mut renderer) = buffer
        .split(Configuration::new(48_000, 2, SampleFormat::I16))
        .unwrap();
    start(&device);
    let input: Vec<u8> = [100i16, -100, 200, -200, 300, -300]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    assert_eq!(device.write_sync(&input[..8], 0), Ok(8));
    let mut output = [0i16; 2];
    assert_eq!(renderer.render_i16(&mut output), 1);
    assert_eq!(output, [100, -100]);
    assert_eq!(device.write_sync(&input[4..], 0), Ok(8));
    let mut output = [0i16; 7];
    assert_eq!(renderer.render_i16(&mut output), 3);
    assert_eq!(output, [200, -200, 200, -200, 300, -300, 0]);
    assert_eq!(device.flush_sync(), Ok(()));
}

#[test]
fn volume_is_applied_when_rendering_and_configuration_is_fixed() {
    let configuration = Configuration::new(48_000, 1, SampleFormat::I16);
    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, mut renderer) = buffer.split(configuration).unwrap();
    let mut actual = Configuration::new(1, 1, SampleFormat::U8);
    control::<GET_CONFIGURATION>(&device, &(), &mut actual).unwrap();
    assert_eq!(actual, configuration);
    control::<SET_CONFIGURATION>(&device, &configuration, &mut ()).unwrap();
    assert_eq!(
        control::<SET_CONFIGURATION>(
            &device,
            &Configuration::new(44_100, 1, SampleFormat::I16),
            &mut (),
        ),
        Err(Error::UnsupportedOperation)
    );
    assert_eq!(
        control::<SET_VOLUME>(&device, &101, &mut ()),
        Err(Error::InvalidParameter)
    );
    device.write_sync(&1000i16.to_le_bytes(), 0).unwrap();
    control::<SET_VOLUME>(&device, &50, &mut ()).unwrap();
    let mut volume = 0;
    control::<GET_VOLUME>(&device, &(), &mut volume).unwrap();
    assert_eq!(volume, 50);
    start(&device);
    let mut output = [0i16; 1];
    assert_eq!(renderer.render_i16(&mut output), 1);
    assert_eq!(output, [500]);
}

#[test]
fn converts_pcm_and_fills_unsigned_silence() {
    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, mut renderer) = buffer
        .split(Configuration::new(48_000, 1, SampleFormat::I16))
        .unwrap();
    start(&device);
    device.write_sync(&(-16384i16).to_le_bytes(), 0).unwrap();
    let mut float = [99.0; 2];
    assert_eq!(renderer.render_f32(&mut float), 1);
    assert_eq!(float, [-0.5, 0.0]);
    let mut unsigned = [0u8; 2];
    assert_eq!(renderer.render_u8(&mut unsigned), 0);
    assert_eq!(unsigned, [128, 128]);
    device.write_sync(&16384i16.to_le_bytes(), 0).unwrap();
    let mut wide = [0i32; 1];
    assert_eq!(renderer.render_i32(&mut wide), 1);
    assert_eq!(wide, [1 << 30]);
}

#[test]
fn backend_failure_is_reported_instead_of_stalling() {
    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, mut renderer) = buffer
        .split(Configuration::new(48_000, 1, SampleFormat::I16))
        .unwrap();
    start(&device);
    device.write_sync(&[0; 2], 0).unwrap();
    renderer.failure_reporter().fail();
    assert_eq!(device.flush_sync(), Err(Error::InputOutput));
    assert_eq!(device.write_sync(&[0; 2], 0), Err(Error::InputOutput));
    assert_eq!(
        control::<START>(&device, &(), &mut ()),
        Err(Error::InputOutput)
    );
    let mut output = [42.0; 2];
    assert_eq!(renderer.render_f32(&mut output), 0);
    assert_eq!(output, [0.0; 2]);
}

#[test]
fn playback_hot_path_has_zero_allocations() {
    let mut buffer = PlaybackBuffer::<4096>::new();
    let (device, mut renderer) = buffer
        .split(Configuration::new(48_000, 2, SampleFormat::I16))
        .unwrap();
    let input = [0x20u8; 1024];
    let mut output = [0.0f32; 512];
    // Warm up the platform critical-section implementation before measuring.
    start(&device);
    TRACKING.with(|tracking| tracking.set(true));
    ALLOCATIONS.with(|count| count.set(0));
    for _ in 0..1000 {
        assert_eq!(device.write_sync(&input, 0), Ok(1024));
        control::<SET_VOLUME>(&device, &50, &mut ()).unwrap();
        assert_eq!(renderer.render_f32(&mut output), 256);
        assert_eq!(device.flush_sync(), Ok(()));
    }
    TRACKING.with(|tracking| tracking.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    println!(
        "4096-byte queue: storage={} B, device={} B, renderer={} B; 1000 cycles: 0 allocations",
        size_of::<PlaybackBuffer<4096>>(),
        size_of_val(&device),
        size_of_val(&renderer)
    );
}

#[test]
fn concurrent_writer_and_callback_preserve_samples() {
    let mut buffer = PlaybackBuffer::<28>::new();
    let (device, mut renderer) = buffer
        .split(Configuration::new(48_000, 1, SampleFormat::I16))
        .unwrap();
    start(&device);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for sample in 1..=5000i16 {
                while device.write_sync(&sample.to_le_bytes(), 0).unwrap() == 0 {
                    std::thread::yield_now();
                }
            }
        });
        let mut expected = 1i16;
        let mut output = [0i16; 11];
        while expected <= 5000 {
            let frames = renderer.render_i16(&mut output);
            for sample in &output[..frames] {
                assert_eq!(*sample, expected);
                expected += 1;
            }
            std::thread::yield_now();
        }
    });
    assert_eq!(device.flush_sync(), Ok(()));
}

#[test]
fn all_input_formats_and_unaligned_bytes_are_supported() {
    for (format, bytes) in [
        (SampleFormat::U8, &[192u8][..]),
        (SampleFormat::I16, &[0, 64][..]),
        (SampleFormat::I24, &[0, 0, 64][..]),
        (SampleFormat::I32, &[0, 0, 0, 64][..]),
        (SampleFormat::F32, &[0, 0, 0, 63][..]),
    ] {
        let mut buffer = PlaybackBuffer::<24>::new();
        let (device, mut renderer) = buffer.split(Configuration::new(48_000, 1, format)).unwrap();
        start(&device);
        // Offset input by one byte, so typed input alignment cannot be assumed.
        let mut input = [0u8; 5];
        input[1..1 + bytes.len()].copy_from_slice(bytes);
        assert_eq!(
            device.write_sync(&input[1..1 + bytes.len()], 0),
            Ok(bytes.len())
        );
        let mut output = [0.0; 1];
        assert_eq!(renderer.render_f32(&mut output), 1);
        assert_eq!(output, [0.5]);
    }
}

#[test]
fn commands_have_unique_audio_identifiers_and_renderer_drop_fails_device() {
    for (index, identifier) in [
        SET_CONFIGURATION::IDENTIFIER,
        GET_CONFIGURATION::IDENTIFIER,
        GET_WRITABLE::IDENTIFIER,
        SET_VOLUME::IDENTIFIER,
        GET_VOLUME::IDENTIFIER,
        START::IDENTIFIER,
        STOP::IDENTIFIER,
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(identifier.get_kind(), b'A');
        assert_eq!(identifier.get_number() as usize, index + 1);
    }
    let mut buffer = PlaybackBuffer::<8>::new();
    let (device, renderer) = buffer
        .split(Configuration::new(48_000, 1, SampleFormat::I16))
        .unwrap();
    drop(renderer);
    assert_eq!(device.write_sync(&[0; 2], 0), Err(Error::InputOutput));
}

#[test]
fn rejects_oversized_misaligned_and_unexpected_command_payloads() {
    use file_system::ControlCommandIdentifier;

    let mut buffer = PlaybackBuffer::<8>::new();
    let configuration = Configuration::new(48_000, 1, SampleFormat::I16);
    let (device, _renderer) = buffer.split(configuration).unwrap();
    assert_eq!(
        device.control_sync(
            SET_CONFIGURATION::IDENTIFIER,
            AnyByLayout::from(&[configuration; 2]),
            AnyByLayout::NONE,
        ),
        Err(Error::InvalidParameter)
    );
    let mut oversized = [0u8; 2];
    assert_eq!(
        device.control_sync(
            GET_VOLUME::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(&mut oversized),
        ),
        Err(Error::InvalidParameter)
    );
    #[repr(align(4))]
    struct Aligned([u8; 16]);
    let mut bytes = Aligned([0; 16]);
    let misaligned: &mut [u8; 12] = (&mut bytes.0[1..13]).try_into().unwrap();
    assert_eq!(
        device.control_sync(
            GET_CONFIGURATION::IDENTIFIER,
            AnyByLayout::from(&()),
            AnyByLayout::from_mutable(misaligned),
        ),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        device.control_sync(
            START::IDENTIFIER,
            AnyByLayout::from(&0u8),
            AnyByLayout::NONE
        ),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        device.control_sync(
            ControlCommandIdentifier::new_read::<(), ()>(b'A', 99),
            AnyByLayout::from(&()),
            AnyByLayout::NONE,
        ),
        Err(Error::UnsupportedOperation)
    );
}
