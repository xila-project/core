#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

#[cfg(any(test, feature = "std", all(feature = "profiling", target_os = "linux")))]
extern crate std;

#[cfg(all(feature = "profiling", target_os = "linux"))]
pub mod profiling;

mod environment_variable;
mod error;
mod join_handle;
mod manager;
mod signal;
mod task;

use core::{future::poll_fn, task::Poll, time::Duration};
use embassy_time::Timer;

pub use embassy_executor;
pub use environment_variable::*;
pub use error::*;
pub use join_handle::*;
pub use manager::*;
pub use signal::*;
pub use task::*;
pub use task_macros::{run, test};

/// Sleep the current thread for a given duration.
pub async fn sleep(duration: impl Into<Duration>) {
    let nano_seconds = duration.into().as_nanos();

    Timer::after(embassy_time::Duration::from_nanos(nano_seconds as u64)).await
}

/// Yield the current thread, allowing other tasks to run.
pub async fn yield_now() {
    #[cfg(target_arch = "wasm32")]
    sleep(Duration::from_millis(10)).await; // Weird behavior in wasm, where the task is not properly yielded without a delay

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
