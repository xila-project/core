//! Wasmi-specific cooperative execution. No guest borrow crosses an await.
use super::{
    error::{Error, Result},
    store::GlobalStore,
    translation::GuestMemory,
    wasi::poll_memory::{ClockPoll, PollError},
};
use alloc::string::ToString;
use core::{fmt, num::NonZeroU32, time::Duration};
use wasmi::{Store, TypedFunc, TypedResumableCall, Val};

pub enum PendingCall {
    Yield,
    ClockPoll {
        poll: ClockPoll,
        memory: wasmi::Memory,
    },
}

#[derive(Debug)]
pub(crate) struct Suspend;
impl fmt::Display for Suspend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cooperative VM suspension")
    }
}
impl wasmi::errors::HostError for Suspend {}

pub(crate) fn clock_now(
    context: &xila::executable::ExecutableContext,
    required: [bool; 2],
) -> core::result::Result<[u64; 2], wasmi::Error> {
    clock_snapshot(context.time_manager, required)
}

pub(crate) fn clock_value(
    manager: &xila::time::Manager<'_>,
    id: usize,
) -> core::result::Result<u64, PollError> {
    match id {
        0 => {
            let realtime = manager.realtime_now().map_err(|_| PollError::Io)?;
            u64::try_from(realtime.as_nanos()).map_err(|_| PollError::Overflow)
        }
        1 => monotonic_now(manager).map_err(|_| PollError::Overflow),
        _ => Err(PollError::NotSupported),
    }
}

fn clock_snapshot(
    manager: &xila::time::Manager<'_>,
    required: [bool; 2],
) -> core::result::Result<[u64; 2], wasmi::Error> {
    let mut now = [0; 2];
    for id in 0..2 {
        if required[id] {
            now[id] = clock_value(manager, id).map_err(|_| wasmi::Error::new("clock error"))?;
        }
    }
    Ok(now)
}

pub(crate) fn monotonic_now(
    manager: &xila::time::Manager<'_>,
) -> core::result::Result<u64, wasmi::Error> {
    // Keep the source's epoch: absolute WASI deadlines and clock_time_get must
    // agree, even when a manager is created after the executor has started.
    u64::try_from(manager.monotonic_now().as_nanos())
        .map_err(|_| wasmi::Error::new("clock overflow"))
}

pub async fn run(
    store: &mut Store<GlobalStore>,
    start: TypedFunc<(), ()>,
    instruction_limit: NonZeroU32,
) -> Result<()> {
    store.set_fuel(instruction_limit.get() as u64)?;
    let mut call = start.call_resumable(&mut *store, ())?;
    loop {
        if super::shutdown_requested() {
            return Err(Error::Interrupted);
        }
        call = match call {
            TypedResumableCall::Finished(()) => return Ok(()),
            TypedResumableCall::OutOfFuel(call) => {
                xila::task::yield_now().await;
                if super::shutdown_requested() {
                    return Err(Error::Interrupted);
                }
                // A single bulk operation may require more than one normal quantum.
                let fuel = u64::from(instruction_limit.get()).max(call.required_fuel());
                store.set_fuel(fuel)?;
                call.resume(&mut *store)?
            }
            TypedResumableCall::HostTrap(call) => {
                if call.host_error().downcast_ref::<Suspend>().is_some() {
                    let pending = store
                        .data_mut()
                        .pending
                        .take()
                        .ok_or_else(|| wasmi::Error::new("missing pending call"))?;
                    let context = store.data().context;
                    let errno = match pending {
                        PendingCall::Yield => {
                            xila::task::yield_now().await;
                            0
                        }
                        PendingCall::ClockPoll { poll, memory } => {
                            let required = poll.required_clocks();
                            loop {
                                if super::shutdown_requested() {
                                    return Err(Error::Interrupted);
                                }
                                let delay = poll.delay(clock_now(context, required)?);
                                if !delay.is_zero() {
                                    // Bound timer and signal polling latency.
                                    xila::task::Manager::sleep(
                                        delay.min(Duration::from_millis(50)),
                                    )
                                    .await;
                                } else {
                                    xila::task::yield_now().await;
                                }
                                let now = clock_now(context, required)?;
                                match poll.complete(
                                    &mut GuestMemory::new(memory.data_mut(&mut *store)),
                                    now,
                                ) {
                                    Ok(0) => continue,
                                    Ok(_) => break 0,
                                    Err(error) => break error as i32,
                                }
                            }
                        }
                    };
                    call.resume(&mut *store, &[Val::I32(errno)])?
                } else if let Some(code) = store.data().wasi.exit_code {
                    return if code == 0 {
                        Ok(())
                    } else {
                        Err(Error::Runtime(code))
                    };
                } else {
                    return Err(Error::Wasm(wasmi::Error::new(
                        call.host_error().to_string(),
                    )));
                }
            }
        };
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use xila::file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};

    struct WallClock(AtomicU64, AtomicBool);
    impl WallClock {
        fn new(seconds: u64) -> Self {
            Self(AtomicU64::new(seconds), AtomicBool::new(false))
        }
    }
    impl DirectBaseOperations for WallClock {
        fn read(
            &self,
            _cx: &mut core::task::Context<'_>,
            buffer: &mut [u8],
            _: Size,
        ) -> core::task::Poll<xila::file_system::Result<usize>> {
            xila::file_system::ready(|| {
                if self.1.load(Ordering::Relaxed) {
                    return Err(xila::file_system::Error::InternalError);
                }
                let time = Duration::from_secs(self.0.load(Ordering::Relaxed));
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&time as *const Duration).cast::<u8>(),
                        size_of::<Duration>(),
                    )
                };
                buffer[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            })
        }
        fn write(
            &self,
            _cx: &mut core::task::Context<'_>,
            _: &[u8],
            _: Size,
        ) -> core::task::Poll<xila::file_system::Result<usize>> {
            xila::file_system::ready(|| Err(xila::file_system::Error::UnsupportedOperation))
        }
    }
    impl MountOperations for WallClock {}
    impl DirectCharacterDevice for WallClock {}

    struct Monotonic(core::sync::atomic::AtomicU64);
    impl xila::time::MonotonicClock for Monotonic {
        fn now(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::Relaxed))
        }
    }

    #[test]
    fn monotonic_clock_is_independent_of_wall_clock_corrections() {
        let device = WallClock::new(1000);
        let clock = Monotonic(AtomicU64::new(500));
        let manager = xila::time::Manager::with_monotonic_clock(&device, &clock).unwrap();
        let first = clock_snapshot(&manager, [true, true]).unwrap();
        device.0.store(2000, Ordering::Relaxed);
        clock.0.store(501, Ordering::Relaxed);
        let forward = clock_snapshot(&manager, [true, true]).unwrap();
        device.0.store(1500, Ordering::Relaxed);
        clock.0.store(502, Ordering::Relaxed);
        let backward = clock_snapshot(&manager, [true, true]).unwrap();
        assert_eq!(first[0], 1_000_000_000_000);
        assert_eq!(backward[0], 1_500_000_000_000);
        assert_eq!(first[1], 500_000_000);
        assert_eq!(forward[1], 501_000_000);
        assert_eq!(backward[1], 502_000_000);
    }

    #[test]
    fn wasi_monotonic_nanoseconds_report_overflow() {
        let device = WallClock::new(1_000);
        let clock = Monotonic(AtomicU64::new(18_446_744_073_710));
        let manager = xila::time::Manager::with_monotonic_clock(&device, &clock).unwrap();
        assert!(clock_snapshot(&manager, [true, true]).is_err());
    }
    #[test]
    fn monotonic_poll_ignores_utc_failure_before_and_after_suspension() {
        use crate::host::{translation::GuestPointer, wasi::poll_memory::Subscription};
        let device = WallClock::new(1_000);
        let clock = Monotonic(AtomicU64::new(500));
        let manager = xila::time::Manager::with_monotonic_clock(&device, &clock).unwrap();
        let mut bytes = [0u8; 256];
        let mut memory = GuestMemory::new(&mut bytes);
        memory
            .write(
                GuestPointer::new(8),
                Subscription {
                    userdata: 42,
                    tag: 0,
                    clock: 1,
                    timeout: 10_000_000,
                    precision: 0,
                    flags: 0,
                },
            )
            .unwrap();
        device.1.store(true, Ordering::Relaxed);
        let poll =
            ClockPoll::prepare(&memory, 8, 64, 1, 200, |id| clock_value(&manager, id)).unwrap();
        let now = clock_snapshot(&manager, poll.required_clocks()).unwrap();
        assert_eq!(poll.delay(now), Duration::from_millis(10));
        assert_eq!(poll.complete(&mut memory, now).unwrap(), 0);
        clock.0.store(510, Ordering::Relaxed);
        let now = clock_snapshot(&manager, poll.required_clocks()).unwrap();
        assert_eq!(poll.delay(now), Duration::ZERO);
        assert_eq!(poll.complete(&mut memory, now).unwrap(), 1);
        let event: crate::host::wasi::poll_memory::PollEvent =
            memory.read(GuestPointer::new(64)).unwrap();
        assert_eq!(event.userdata, 42);
    }
}
