use audio::{AudioSystem, Configuration, SampleFormat};
use file_system::Error;
use std::{
    alloc::{GlobalAlloc, Layout, System as NativeAllocator},
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
        unsafe { NativeAllocator.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { NativeAllocator.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { NativeAllocator.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { NativeAllocator.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

type System = AudioSystem<2, 32, 2, 2, 48>;

fn configuration() -> Configuration {
    Configuration::new(48_000, 2, SampleFormat::F32)
}

#[test]
fn stream_slots_are_bounded_and_reused_without_old_pcm() {
    let system = core::pin::pin!(System::new().unwrap());
    let first = system.open_stream().unwrap();
    let second = system.open_stream().unwrap();
    assert!(matches!(system.open_stream(), Err(Error::RessourceBusy)));
    assert_eq!(first.configuration(), configuration());
    assert_eq!(first.write(&[0; 8]), Ok(8));
    drop(first);
    let replacement = system.open_stream().unwrap();
    assert_eq!(replacement.writable(), 32);
    assert_eq!(second.writable(), 32);
}

#[test]
fn writes_are_partial_whole_frames_and_reject_incomplete_frames() {
    let system = core::pin::pin!(System::new().unwrap());
    let stream = system.open_stream().unwrap();
    assert_eq!(stream.write(&[1; 7]), Err(Error::InvalidParameter));
    assert_eq!(stream.writable(), 32);
    assert_eq!(stream.write(&[1; 24]), Ok(24));
    assert_eq!(stream.write(&[2; 24]), Ok(8));
    assert_eq!(stream.write(&[3; 8]), Ok(0));
}

#[test]
fn invalid_capacities_are_rejected() {
    assert!(AudioSystem::<2, 0, 2, 2, 48>::new().is_err());
    assert!(AudioSystem::<2, 31, 2, 2, 48>::new().is_err());
    assert!(AudioSystem::<2, 32, 0, 2, 48>::new().is_err());
    assert!(AudioSystem::<2, 32, 2, 2, 8>::new().is_err());
    assert!(AudioSystem::<2, 32, 2, 2, 47>::new().is_err());
    assert!(AudioSystem::<2, 32, { usize::MAX }, 2, 48>::new().is_err());
}

#[test]
fn outputs_are_bounded_and_reused_with_empty_consumers() {
    let system = core::pin::pin!(System::new().unwrap());
    let mut first = system.as_ref().open_output().unwrap();
    let second = system.as_ref().open_output().unwrap();
    assert!(matches!(
        system.as_ref().open_output(),
        Err(Error::RessourceBusy)
    ));
    assert_eq!(first.configuration(), configuration());
    assert_eq!(first.with_consumer(|consumer| consumer.available()), 0);
    drop(first);
    assert_eq!(
        system
            .as_ref()
            .open_output()
            .unwrap()
            .with_consumer(|consumer| consumer.available()),
        0
    );
    drop(second);
}

fn pcm(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

#[test]
fn mixes_independent_streams_once_and_fans_out_identical_blocks() {
    let system = core::pin::pin!(System::new().unwrap());
    let first = system.open_stream().unwrap();
    let second = system.open_stream().unwrap();
    let mut left = system.as_ref().open_output().unwrap();
    let mut right = system.as_ref().open_output().unwrap();
    first.write(&pcm(&[0.25, -0.25, 0.125, -0.125])).unwrap();
    second.write(&pcm(&[0.125, 0.125, 0.25, 0.25])).unwrap();
    first.start().unwrap();
    second.start().unwrap();
    system.as_ref().mix_next_block().unwrap();
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    assert_eq!(left.with_consumer(|consumer| consumer.read(&mut a)), 16);
    assert_eq!(right.with_consumer(|consumer| consumer.read(&mut b)), 16);
    assert_eq!(a.as_slice(), pcm(&[0.375, -0.125, 0.375, 0.125]));
    assert_eq!(a, b);
    assert_eq!(first.writable(), 32);
    assert_eq!(second.writable(), 32);
}

#[test]
fn stream_and_master_gain_are_combined_and_reset_on_reuse() {
    let system = core::pin::pin!(System::new().unwrap());
    let first = system.open_stream().unwrap();
    let second = system.open_stream().unwrap();
    let mut output = system.as_ref().open_output().unwrap();
    assert_eq!(first.volume(), Ok(100));
    first.set_volume(50).unwrap();
    assert_eq!(first.volume(), Ok(50));
    assert_eq!(first.set_volume(101), Err(Error::InvalidParameter));
    system.set_master_volume(50).unwrap();
    assert_eq!(system.set_master_volume(101), Err(Error::InvalidParameter));
    first.write(&pcm(&[1.0; 4])).unwrap();
    second.write(&pcm(&[0.5; 4])).unwrap();
    first.start().unwrap();
    second.start().unwrap();
    system.as_ref().mix_next_block().unwrap();
    let mut bytes = [0; 16];
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes.as_slice(), pcm(&[0.5; 4]));
    drop(first);
    assert_eq!(system.open_stream().unwrap().volume(), Ok(100));
}

#[test]
fn stopped_and_empty_streams_are_silent_and_stop_retains_pcm() {
    let system = core::pin::pin!(System::new().unwrap());
    let stream = system.open_stream().unwrap();
    let mut output = system.as_ref().open_output().unwrap();
    stream.write(&pcm(&[0.25, -0.25])).unwrap();
    assert_eq!(stream.flush(), Err(Error::RessourceBusy));
    let mut bytes = [99; 16];
    system.as_ref().mix_next_block().unwrap();
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes, [0; 16]);
    assert_eq!(stream.writable(), 24);
    stream.start().unwrap();
    stream.stop().unwrap();
    system.as_ref().mix_next_block().unwrap();
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes, [0; 16]);
    stream.start().unwrap();
    system.as_ref().mix_next_block().unwrap();
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes.as_slice(), pcm(&[0.25, -0.25, 0.0, 0.0]));
    system.as_ref().mix_next_block().unwrap();
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes, [0; 16]);
    assert_eq!(stream.flush(), Ok(()));
}

#[test]
fn full_output_drops_whole_blocks_without_stalling_other_outputs() {
    let system = core::pin::pin!(System::new().unwrap());
    let stream = system.open_stream().unwrap();
    let mut slow = system.as_ref().open_output().unwrap();
    let mut fast = system.as_ref().open_output().unwrap();
    stream.start().unwrap();
    for value in [0.125, 0.25, 0.375, 0.5, 0.625] {
        assert_eq!(stream.write(&pcm(&[value; 4])), Ok(16));
        system.as_ref().mix_next_block().unwrap();
        let mut bytes = [0; 16];
        assert_eq!(fast.with_consumer(|consumer| consumer.read(&mut bytes)), 16);
        assert_eq!(bytes.as_slice(), pcm(&[value; 4]));
        assert_eq!(stream.writable(), 32);
    }
    assert_eq!(slow.dropped_blocks(), 2);
    assert_eq!(fast.dropped_blocks(), 0);
    assert_eq!(slow.with_consumer(|consumer| consumer.available()), 48);
    let mut bytes = [0; 48];
    assert_eq!(slow.with_consumer(|consumer| consumer.read(&mut bytes)), 48);
    assert_eq!(
        bytes.as_slice(),
        pcm(&[0.125; 4])
            .into_iter()
            .chain(pcm(&[0.25; 4]))
            .chain(pcm(&[0.375; 4]))
            .collect::<Vec<_>>()
    );
    stream.write(&pcm(&[0.75; 4])).unwrap();
    system.as_ref().mix_next_block().unwrap();
    let mut bytes = [0; 16];
    slow.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes.as_slice(), pcm(&[0.75; 4]));
    drop(slow);
    let mut replacement = system.as_ref().open_output().unwrap();
    assert_eq!(replacement.dropped_blocks(), 0);
    assert_eq!(
        replacement.with_consumer(|consumer| consumer.available()),
        0
    );
}

#[test]
fn default_capacity_hot_paths_allocate_nothing() {
    let system: AudioSystem = AudioSystem::new().unwrap();
    let system = core::pin::pin!(system);
    let streams = core::array::from_fn::<_, 4, _>(|_| system.open_stream().unwrap());
    let mut outputs = core::array::from_fn::<_, 2, _>(|_| system.as_ref().open_output().unwrap());
    let input = [0; 1024];
    let mut output = [0; 1024];
    for stream in &streams {
        stream.start().unwrap();
    }
    ALLOCATIONS.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
    for _ in 0..1000 {
        for stream in &streams {
            assert_eq!(stream.write(&input), Ok(input.len()));
            stream.set_volume(50).unwrap();
        }
        system.set_master_volume(50).unwrap();
        system.as_ref().mix_next_block().unwrap();
        for sink in &mut outputs {
            assert_eq!(
                sink.with_consumer(|consumer| consumer.read(&mut output)),
                output.len()
            );
        }
    }
    TRACKING.with(|tracking| tracking.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    let pcm_bytes = size_of::<[[u8; 2048]; 4]>() + size_of::<[[u8; 3072]; 2]>();
    assert_eq!(pcm_bytes, 14 * 1024);
    println!(
        "default system={} B, stream handle={} B, output handle={} B, queue PCM={} B; 1000 cycles, 0 allocations",
        size_of_val(system.as_ref().get_ref()),
        size_of_val(&streams[0]),
        size_of_val(&outputs[0]),
        14 * 1024
    );
}

#[test]
fn static_output_can_transfer_its_consumer_to_an_owning_platform_adapter() {
    let system = Box::leak(Box::new(Box::pin(System::new().unwrap())));
    let system = core::pin::Pin::as_ref(system);
    let mut consumer = system.open_output().unwrap().into_consumer();
    let other = system.open_output().unwrap();
    assert!(matches!(system.open_output(), Err(Error::RessourceBusy)));
    system.mix_next_block().unwrap();
    let mut bytes = [99; 16];
    assert_eq!(consumer.read(&mut bytes), 16);
    assert_eq!(bytes, [0; 16]);
    drop(other);
}

#[test]
fn static_output_reservation_releases_only_after_consumer_drop_and_lease_drop() {
    let system = Box::leak(Box::new(Box::pin(System::new().unwrap())));
    let system = core::pin::Pin::as_ref(system);
    let (reservation, second) = {
        let (_consumer, reservation) = unsafe {
            system
                .open_output()
                .unwrap()
                .into_consumer_with_reservation()
        };
        let second = system.open_output().unwrap();
        assert!(matches!(system.open_output(), Err(Error::RessourceBusy)));
        (reservation, second)
    };
    assert!(matches!(system.open_output(), Err(Error::RessourceBusy)));
    drop(reservation);
    let third = system.open_output().unwrap();
    drop((second, third));
}

#[test]
fn unaligned_writes_preserve_frame_order_across_queue_wrap() {
    let system = core::pin::pin!(System::new().unwrap());
    let stream = system.open_stream().unwrap();
    let mut output = system.as_ref().open_output().unwrap();
    let mut input = [0; 25];
    input[1..].copy_from_slice(&pcm(&[0.125, -0.125, 0.25, -0.25, 0.375, -0.375]));
    assert_eq!(stream.write(&input[1..]), Ok(24));
    stream.start().unwrap();
    system.as_ref().mix_next_block().unwrap();
    let mut bytes = [0; 16];
    output.with_consumer(|consumer| consumer.read(&mut bytes));
    assert_eq!(bytes.as_slice(), pcm(&[0.125, -0.125, 0.25, -0.25]));
    input[1..].copy_from_slice(&pcm(&[0.5, -0.5, 0.625, -0.625, 0.75, -0.75]));
    assert_eq!(stream.write(&input[1..]), Ok(24));
    for expected in [[0.375, -0.375, 0.5, -0.5], [0.625, -0.625, 0.75, -0.75]] {
        system.as_ref().mix_next_block().unwrap();
        output.with_consumer(|consumer| consumer.read(&mut bytes));
        assert_eq!(bytes.as_slice(), pcm(&expected));
    }
}

#[test]
fn forgotten_output_keeps_its_pinned_slot_reserved() {
    let system = core::pin::pin!(System::new().unwrap());
    core::mem::forget(system.as_ref().open_output().unwrap());
    let _remaining = system.as_ref().open_output().unwrap();
    assert!(matches!(
        system.as_ref().open_output(),
        Err(Error::RessourceBusy)
    ));
    for _ in 0..10 {
        system.as_ref().mix_next_block().unwrap();
    }
}

#[test]
fn mixer_configuration_is_fixed_48000_stereo_f32() {
    let system = core::pin::pin!(System::new().unwrap());
    let stream = system.open_stream().unwrap();
    let output = system.as_ref().open_output().unwrap();
    let expected = Configuration::new(48_000, 2, SampleFormat::F32);
    assert_eq!(stream.configuration(), expected);
    assert_eq!(output.configuration(), expected);
}
