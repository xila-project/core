use crate::manager::stack::Stack;
use core::time::Duration;
use embassy_futures::select::select;
use smoltcp::phy::Device;
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

/// A signal used to wake the runner when socket activity occurs.
pub type WakeSignal = Arc<Signal<CriticalSectionRawMutex, ()>>;

pub struct StackRunner<T> {
    stack: Stack,
    device: T,
    wake_signal: WakeSignal,
}

const MAX_POLL_INTERVAL: Duration = Duration::from_millis(20);

impl<T> StackRunner<T>
where
    T: Device,
{
    pub fn new(stack: Stack, device: T, wake_signal: WakeSignal) -> Self {
        Self {
            stack,
            device,
            wake_signal,
        }
    }

    pub async fn run(&mut self) -> ! {
        loop {
            let next_poll_in = self
                .stack
                .with_mutable_no_wake(|stack_inner| {
                    if stack_inner.enabled {
                        stack_inner.poll(&mut self.device)
                    } else {
                        None
                    }
                })
                .await;

            let sleep_duration = match next_poll_in {
                Some(d) if d.is_zero() => {
                    embassy_futures::yield_now().await;
                    continue;
                }
                Some(d) => d.min(MAX_POLL_INTERVAL),
                None => MAX_POLL_INTERVAL,
            };

            select(task::sleep(sleep_duration), self.wake_signal.wait()).await;
        }
    }
}
