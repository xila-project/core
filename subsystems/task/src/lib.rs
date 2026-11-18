#![no_std]

extern crate alloc;

embedded_i18n::i18n!("locales");

#[cfg(any(
    test,
    feature = "std",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    target_arch = "wasm32"
))]
extern crate std;

#[cfg(all(feature = "profiling", target_os = "linux"))]
pub mod profiling;

#[cfg(any(test, feature = "test_harness"))]
pub mod test_executor;

pub mod metrics;

mod environment_variable;
mod error;
mod join_handle;
mod manager;
mod signal;
mod task;

use core::{future::poll_fn, task::Poll, time::Duration};
use embassy_time::{Instant, Timer};

pub use embassy_executor;
pub use environment_variable::*;
pub use error::*;
pub use join_handle::*;
pub use manager::*;
pub use signal::*;
pub use task::*;
pub use task_macros::{run, test};

/// The Embassy instant at which a wait of `nanoseconds` has fully elapsed.
///
/// `embassy_time::Duration::from_nanos` adds 999 before dividing, which wraps for
/// huge values (a debug build panics and a release build returns at once), and
/// `Timer::after` starts from the current tick rounded down, so a wait could end
/// up to a tick early. This rounds the ticks up, adds one tick to every non-zero
/// wait and saturates, so `u64::MAX` waits practically forever.
pub fn deadline_after_nanoseconds(nanoseconds: u64) -> Instant {
    const fn gcd(a: u64, b: u64) -> u64 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    const DIVISOR: u64 = gcd(embassy_time::TICK_HZ, 1_000_000_000);
    const TICKS: u64 = embassy_time::TICK_HZ / DIVISOR;
    const NANOSECONDS: u64 = 1_000_000_000 / DIVISOR;
    // The remainder is below `NANOSECONDS`, so its product with `TICKS` must fit.
    const _: () = assert!(NANOSECONDS.checked_mul(TICKS).is_some());

    let ticks = if nanoseconds == 0 {
        0
    } else {
        let whole = (nanoseconds / NANOSECONDS).saturating_mul(TICKS);
        let part = (nanoseconds % NANOSECONDS * TICKS).div_ceil(NANOSECONDS);
        whole.saturating_add(part).saturating_add(1)
    };
    Instant::from_ticks(Instant::now().as_ticks().saturating_add(ticks))
}

/// Sleep the current thread for a given duration.
pub async fn sleep(duration: impl Into<Duration>) {
    let nanoseconds = u64::try_from(duration.into().as_nanos()).unwrap_or(u64::MAX);

    Timer::at(deadline_after_nanoseconds(nanoseconds)).await
}

/// Yield the current thread, allowing other tasks to run.
pub async fn yield_now() {
    #[cfg(target_arch = "wasm32")]
    // Embassy 0.10's wasm pender uses Promise.then (a microtask). A
    // self-waking yield repeatedly schedules microtasks, starving browser
    // rendering, input and timer macrotasks. Keep a timer-backed yield until
    // the platform executor provides a macrotask scheduling primitive. The
    // 10 ms policy is retained pending browser latency/fairness validation.
    sleep(Duration::from_millis(10)).await;

    #[cfg(not(target_arch = "wasm32"))]
    embassy_futures::yield_now().await;
}

/// Suspend the current task until the provided function is called with a context.
/// The function will be called with a mutable reference to the task's context, allowing it to wake the
pub async fn suspend(function: impl FnOnce(&mut core::task::Context<'_>)) {
    let mut function_opt = Some(function);

    poll_fn(|context| {
        if let Some(f) = function_opt.take() {
            f(context);
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
}

pub fn block_on<F: core::future::Future>(future: F) -> F::Output {
    embassy_futures::block_on(future)
}

#[cfg(test)]
mod tests {
    use super::{Instant, deadline_after_nanoseconds};

    #[test]
    fn deadline_rounds_up_and_saturates() {
        let now = Instant::now().as_ticks();
        assert!(deadline_after_nanoseconds(0).as_ticks() >= now);
        // Every non-zero wait ends after the tick it started in.
        assert!(deadline_after_nanoseconds(1).as_ticks() > now);
        // A wait of one second needs at least `TICK_HZ` ticks.
        assert!(
            deadline_after_nanoseconds(1_000_000_000).as_ticks() >= now + embassy_time::TICK_HZ
        );
        // The largest requests neither wrap nor panic.
        let far = deadline_after_nanoseconds(u64::MAX).as_ticks();
        assert!(far > deadline_after_nanoseconds(u64::MAX / 2).as_ticks());
        assert!(far > now + embassy_time::TICK_HZ);
    }
}
