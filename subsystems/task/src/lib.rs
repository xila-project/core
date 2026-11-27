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

#[cfg(feature = "crash")]
pub mod active_task;

pub mod metrics;

mod environment_variable;
mod error;
mod join_handle;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod macrotask;
mod manager;
mod signal;
mod task;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod time_driver;

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

/// How long [`yield_throttled`] sleeps on WebAssembly targets.
#[cfg(target_arch = "wasm32")]
const THROTTLED_YIELD: Duration = Duration::from_millis(10);

/// Yield the current thread, allowing other tasks to run.
///
/// The task is rescheduled as soon as the other ready tasks have run. Use this
/// to slice long computations into quanta. A task that polls for an event
/// should call [`yield_throttled`] instead, so that it does not keep a
/// single-threaded host busy.
///
/// On `wasm32-unknown-unknown` this ends the current browser task: timers,
/// input and rendering that are already due run before the task resumes, and it
/// costs far less than a millisecond. Embassy's own wasm yield is a microtask
/// that would starve all of them, and a timer-backed yield costs 10 ms or more
/// per call, which throttles anything that yields often (the Wasm runtime
/// yields once per fuel quantum, so a timer made audio rendering choppy).
/// Where the browser exposes no `MessageChannel`, it falls back to a zero-delay
/// timer, and where there is no timer either (an `AudioWorkletGlobalScope`) to
/// a microtask. The microtask fallback does not let the host run: a loop that
/// only yields starves everything else in its scope, so such a scope must wait
/// on something other than `yield_now` (and cannot use [`sleep`], which needs a
/// timer). On `wasm32-wasip1` it sleeps 10 ms: there is no host event loop to
/// give control to.
pub async fn yield_now() {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    macrotask::yield_now().await;

    // Embassy's wasm pender uses `Promise.then` (a microtask), so a self-waking
    // yield cannot let the host run. Keep a timer-backed yield.
    #[cfg(all(target_arch = "wasm32", not(target_os = "unknown")))]
    sleep(THROTTLED_YIELD).await;

    #[cfg(not(target_arch = "wasm32"))]
    embassy_futures::yield_now().await;
}

/// Yield the current thread, but never resume faster than a polling loop should.
///
/// For guest-initiated busy-waits (`sched_yield`, a zero-delay poll) whose
/// caller makes no progress until something else changes: such a loop must not
/// peg a single-threaded host at 100 %. On WebAssembly targets this sleeps
/// 10 ms (the timer-backed yield [`yield_now`] used to be); elsewhere it is
/// [`yield_now`], since the scheduler there already runs other tasks and idles
/// when none is ready.
///
/// Do not use it to slice work that is making progress, because the wait is
/// not cheap: use [`yield_now`].
pub async fn yield_throttled() {
    #[cfg(target_arch = "wasm32")]
    sleep(THROTTLED_YIELD).await;

    #[cfg(not(target_arch = "wasm32"))]
    yield_now().await;
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
