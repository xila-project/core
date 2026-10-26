#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

xila::internationalization::track_locales!();

mod interface;
mod model;
mod workload;

#[cfg(all(test, target_os = "linux"))]
drivers_std::memory::instantiate_global_allocator!();

use alloc::{boxed::Box, string::String, vec::Vec};
use core::{num::NonZeroUsize, time::Duration};
use interface::{Action, View};
use model::{Config, MemorySample};
use workload::Workload;
use xila::executable::{self, ExecutableTrait, Standard};

pub struct ActivityExecutable;
impl ExecutableTrait for ActivityExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(run(standard, arguments))
    }
}

async fn run(standard: Standard<'static>, arguments: Vec<String>) -> Result<(), NonZeroUsize> {
    let mut config = Config::parse(&arguments).map_err(|_| code(2))?;
    let context = standard.context;
    let graphics = context.graphics_manager.ok_or(code(1))?;
    let mut view = View::new(graphics).await?;
    let mut workload: Option<Workload> = None;
    let mut failed = false;
    let mut shown_controls = None;
    let mut next_sample = Duration::ZERO;
    loop {
        if workload
            .as_mut()
            .is_some_and(|running| !running.tick(context.time_manager.uptime()))
        {
            workload = None;
        }
        if let Some(action) = view.action().await {
            match action {
                Action::Close => {
                    return Ok(());
                }
                Action::ToggleWorkload => {
                    failed = false;
                    if workload.take().is_none() {
                        match Workload::new(context.time_manager.uptime()) {
                            Ok(running) => workload = Some(running),
                            Err(()) => failed = true,
                        }
                    }
                    next_sample = Duration::ZERO;
                }
                Action::Interval => {
                    config.interval_ms = match config.interval_ms {
                        250 => 1000,
                        1000 => 2000,
                        _ => 250,
                    };
                    next_sample = Duration::ZERO;
                }
            }
        }
        let now = context.time_manager.uptime();
        if now >= next_sample {
            // Provider awaits occur without holding the UI lock.
            let count = context.task_manager.get_task_count().await;
            let memory = xila::memory::global_manager();
            let memory = MemorySample::new(memory.get_used(), memory.get_total_size());
            view.refresh(now.as_secs(), count, memory).await;
            next_sample = now + Duration::from_millis(config.interval_ms);
        }
        let controls = (config.interval_ms, workload.is_some(), failed);
        if shown_controls != Some(controls) {
            view.controls(controls.0, controls.1, controls.2).await;
            shown_controls = Some(controls);
        }
        let next_wake = workload
            .as_ref()
            .map_or(next_sample, |running| next_sample.min(running.next_wake()));
        let delay = model::bounded_wait(next_wake, context.time_manager.uptime());
        view.wait(delay).await;
    }
}

fn code(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}
