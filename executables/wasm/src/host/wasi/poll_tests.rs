use super::poll_memory::{ClockPoll, PollError, PollEvent, Subscription};
use crate::host::translation::{GuestMemory, GuestPointer, GuestValue, WasmUsize};
use alloc::vec;

fn subscription(timeout: u64, clock: u32, flags: u16) -> Subscription {
    Subscription {
        userdata: 0x8877665544332211,
        tag: 0,
        clock,
        timeout,
        precision: 7,
        flags,
    }
}

#[test]
fn clock_poll_records_use_independent_le_fixtures_and_zero_padding() {
    let mut fixture = [0xff; 48];
    fixture[..8].copy_from_slice(&0x8877665544332211_u64.to_le_bytes());
    fixture[8] = 0;
    fixture[16..20].copy_from_slice(&1_u32.to_le_bytes());
    fixture[24..32].copy_from_slice(&20_u64.to_le_bytes());
    fixture[32..40].copy_from_slice(&7_u64.to_le_bytes());
    fixture[40..42].copy_from_slice(&1_u16.to_le_bytes());
    let value = Subscription::decode(&fixture);
    assert_eq!(value, subscription(20, 1, 1));
    let mut output = [0xff; 48];
    value.encode(&mut output);
    for range in [9..16, 20..24, 42..48] {
        assert!(output[range].iter().all(|b| *b == 0));
    }
    let event = PollEvent {
        userdata: 0x8877665544332211,
    };
    let mut bytes = [0xff; 32];
    event.encode(&mut bytes);
    assert_eq!(&bytes[..8], &fixture[..8]);
    assert!(bytes[8..].iter().all(|b| *b == 0));
    assert_eq!(PollEvent::decode(&bytes), event);
}

#[test]
fn poll_waits_for_earliest_deadline_and_writes_only_ready_events() {
    let mut bytes = [0xaa; 300];
    let mut memory = GuestMemory::new(&mut bytes);
    memory
        .write(GuestPointer::new(1), subscription(10, 0, 0))
        .unwrap();
    memory
        .write(GuestPointer::new(49), subscription(25, 1, 1))
        .unwrap();
    let poll = ClockPoll::prepare(&memory, 1, 101, 2, 201, |_| Ok(10)).unwrap();
    assert_eq!(poll.delay([10, 10]).as_nanos(), 10);
    assert_eq!(poll.complete(&mut memory, [19, 19]).unwrap(), 0);
    assert_eq!(memory.read(GuestPointer::<WasmUsize>::new(201)), Some(0));
    assert_eq!(poll.complete(&mut memory, [20, 20]).unwrap(), 1);
    assert_eq!(
        memory.read(GuestPointer::<PollEvent>::new(101)),
        Some(PollEvent {
            userdata: 0x8877665544332211
        })
    );
    assert_eq!(poll.complete(&mut memory, [25, 25]).unwrap(), 2);
    // Both outputs may overlap; values are written in a defined sequential order.
    let overlap = ClockPoll::prepare(&memory, 1, 101, 1, 101, |_| Ok(0)).unwrap();
    assert_eq!(overlap.complete(&mut memory, [100, 100]).unwrap(), 1);
    assert_eq!(memory.read(GuestPointer::<WasmUsize>::new(101)), Some(1));
}

#[test]
fn polling_rejects_invalid_requests_without_output_side_effects() {
    let mut bytes = [0xaa; 4096];
    let mut memory = GuestMemory::new(&mut bytes);
    memory
        .write(GuestPointer::new(1), subscription(1, 0, 0))
        .unwrap();
    for (input, events, count, written, errno) in [
        (0, 100, 1, 200, PollError::Fault),
        (1, 0, 1, 200, PollError::Fault),
        (1, 100, 1, 0, PollError::Fault),
        (1, 100, 0, 200, PollError::Invalid),
        (1, 100, 65, 200, PollError::Invalid),
        (WasmUsize::MAX, 100, 1, 200, PollError::Fault),
        (1, WasmUsize::MAX, 1, 200, PollError::Fault),
        (1, 100, 1, WasmUsize::MAX, PollError::Fault),
    ] {
        assert_eq!(
            ClockPoll::prepare(&memory, input, events, count, written, |_| Ok(0)).err(),
            Some(errno)
        );
    }
    for (mut value, errno) in [
        (subscription(1, 2, 0), PollError::NotSupported),
        (subscription(1, 0, 2), PollError::Invalid),
        (subscription(u64::MAX, 0, 0), PollError::Overflow),
        (subscription(1, 0, 0), PollError::NotSupported),
    ] {
        if errno == PollError::NotSupported && value.clock == 0 {
            value.tag = 1;
        }
        memory.write(GuestPointer::new(1), value).unwrap();
        assert_eq!(
            ClockPoll::prepare(&memory, 1, 100, 1, 200, |_| Ok(1)).err(),
            Some(errno)
        );
    }
    assert_eq!(memory.read(GuestPointer::<u8>::new(100)), Some(0xaa));
    assert_eq!(memory.read(GuestPointer::<u8>::new(200)), Some(0xaa));
    let mut tiny_bytes = [0; 1];
    let mut tiny = GuestMemory::new(&mut tiny_bytes);
    memory
        .write(GuestPointer::new(1), subscription(1, 0, 0))
        .unwrap();
    let poll = ClockPoll::prepare(&memory, 1, 100, 1, 200, |_| Ok(0)).unwrap();
    assert_eq!(poll.complete(&mut tiny, [1, 1]), Err(PollError::Fault));
    let mut full = vec![0; 4096];
    let mut full = GuestMemory::new(&mut full);
    for i in 0..64 {
        full.write(GuestPointer::new(1 + 48 * i), subscription(0, 0, 0))
            .unwrap();
    }
    assert_eq!(
        ClockPoll::prepare(&full, 1, 3100, 64, 3080, |_| Ok(0)).err(),
        Some(PollError::Fault)
    );
    // A full output table needs its own memory, not truncation to one event.
    let mut full = vec![0; 8192];
    let mut full = GuestMemory::new(&mut full);
    for i in 0..64 {
        full.write(GuestPointer::new(1 + 48 * i), subscription(0, 0, 0))
            .unwrap();
    }
    let poll = ClockPoll::prepare(&full, 1, 3100, 64, 3080, |_| Ok(0)).unwrap();
    assert_eq!(poll.complete(&mut full, [0, 0]).unwrap(), 64);
}
