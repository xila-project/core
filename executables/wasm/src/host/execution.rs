//! Wasmi-specific cooperative execution. No guest borrow crosses an await.
use super::{
    error::{Error, Result},
    store::GlobalStore,
    translation::GuestMemory,
    wasi::poll_memory::ClockPoll,
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
) -> core::result::Result<[u64; 2], wasmi::Error> {
    clock_snapshot(context.time_manager)
}

fn clock_snapshot(
    manager: &xila::time::Manager<'_>,
) -> core::result::Result<[u64; 2], wasmi::Error> {
    let realtime = manager
        .get_current_time()
        .map_err(|_| wasmi::Error::new("clock error"))?;
    Ok([
        u64::try_from(realtime.as_nanos()).map_err(|_| wasmi::Error::new("clock overflow"))?,
        monotonic_now(),
    ])
}

pub(crate) fn monotonic_now() -> u64 {
    // Use the executor's platform clock, not elapsed civil time. This also
    // keeps absolute WASI poll deadlines in the same epoch as clock_time_get.
    embassy_time::Instant::now().as_nanos()
}

pub async fn run(
    store: &mut Store<GlobalStore>,
    start: TypedFunc<(), ()>,
    instruction_limit: NonZeroU32,
) -> Result<()> {
    store.set_fuel(instruction_limit.get() as u64)?;
    let mut call = start.call_resumable(&mut *store, ())?;
    loop {
        if super::shutdown_requested() || xila::lifecycle::SYSTEM.is_shutdown_requested() {
            return Err(Error::Interrupted);
        }
        call = match call {
            TypedResumableCall::Finished(()) => return Ok(()),
            TypedResumableCall::OutOfFuel(call) => {
                xila::task::yield_now().await;
                if super::shutdown_requested() || xila::lifecycle::SYSTEM.is_shutdown_requested() {
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
                            loop {
                                if super::shutdown_requested() || xila::lifecycle::SYSTEM.is_shutdown_requested() {
                                    return Err(Error::Interrupted);
                                }
                                let delay = poll.delay(clock_now(context)?);
                                if !delay.is_zero() {
                                    // Bound timer and signal polling latency.
                                    xila::task::Manager::sleep(
                                        delay.min(Duration::from_millis(50)),
                                    )
                                    .await;
                                } else {
                                    xila::task::yield_now().await;
                                }
                                let now = clock_now(context)?;
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
    use core::sync::atomic::{AtomicU64, Ordering};
    use xila::file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};

    struct WallClock(AtomicU64);
    impl DirectBaseOperations for WallClock {
        fn read(
            &self,
            _cx: &mut core::task::Context<'_>,
            buffer: &mut [u8],
            _: Size,
        ) -> core::task::Poll<xila::file_system::Result<usize>> {
            xila::file_system::ready(|| {
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

    #[test]
    fn monotonic_clock_is_independent_of_wall_clock_corrections() {
        let device = WallClock(AtomicU64::new(1000));
        let manager = xila::time::Manager::new(&device).unwrap();
        let first = clock_snapshot(&manager).unwrap();
        device.0.store(2000, Ordering::Relaxed);
        let forward = clock_snapshot(&manager).unwrap();
        device.0.store(1500, Ordering::Relaxed);
        let backward = clock_snapshot(&manager).unwrap();
        assert_eq!(first[0], 1_000_000_000_000);
        assert_eq!(backward[0], 1_500_000_000_000);
        assert!(forward[1] >= first[1]);
        assert!(
            backward[1] >= forward[1],
            "wall-clock corrections must not rewind timers"
        );
        assert!(
            backward[1] - first[1] < 1_000_000_000,
            "wall-clock jumps must not advance timers"
        );
    }
}
