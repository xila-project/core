//! Optional Linux task poll accounting. CPU time excludes time between polls.

use alloc::{string::String, sync::Arc, vec::Vec};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::{sync::Mutex, time::Instant};

std::thread_local! { static CURRENT: core::cell::RefCell<Option<Arc<Counters>>> = const { core::cell::RefCell::new(None) }; }

static TASKS: Mutex<Vec<Arc<Counters>>> = Mutex::new(Vec::new());

struct Counters {
    generation: usize,
    manager: usize,
    task: usize,
    name: String,
    spawner: usize,
    polls: AtomicU64,
    cpu: AtomicU64,
    wall: AtomicU64,
    max_wall: AtomicU64,
    clock_errors: AtomicU64,
    completed: AtomicBool,
    poll_active: AtomicBool,
    allocations: AtomicU64,
    allocated_bytes: AtomicU64,
    thread_id: u64,
}

/// A cumulative sample. Completed generations remain available until process exit.
pub struct TaskSample {
    pub generation: usize,
    pub manager: usize,
    pub task: usize,
    pub name: String,
    pub spawner: usize,
    pub polls: u64,
    pub cpu_nanoseconds: u64,
    pub poll_wall_nanoseconds: u64,
    pub max_poll_wall_nanoseconds: u64,
    pub clock_errors: u64,
    pub completed: bool,
    pub poll_active: bool,
    pub allocation_requests: u64,
    pub allocation_request_bytes: u64,
    pub thread_id: u64,
}

struct ActivePoll<'a>(&'a AtomicBool);
impl Drop for ActivePoll<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

struct CurrentGuard(Option<Arc<Counters>>);
impl Drop for CurrentGuard {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            current.replace(self.0.take());
        });
    }
}

/// Attribute successful native-manager requests to the currently polled task.
/// This counts requested volume, not retained bytes or allocator overhead.
pub fn record_allocation(size: usize) {
    // Global allocation can also happen after this thread's profiling TLS drops.
    let _ = CURRENT.try_with(|current| {
        if let Ok(current) = current.try_borrow()
            && let Some(counters) = current.as_ref()
        {
            counters.allocations.fetch_add(1, Ordering::Relaxed);
            counters
                .allocated_bytes
                .fetch_add(size as u64, Ordering::Relaxed);
        }
    });
}

/// Current OS-thread CPU time, not elapsed time or whole-process CPU time.
pub fn thread_cpu_nanoseconds() -> Option<u64> {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut value) } != 0 {
        return None;
    }
    Some(value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64)
}

/// Poll a future while retaining its task identity and selected spawner.
pub async fn instrument<F: Future>(
    manager: usize,
    task: usize,
    name: &str,
    spawner: usize,
    future: F,
) -> F::Output {
    let counters = {
        let mut tasks = TASKS.lock().unwrap();
        let counters = Arc::new(Counters {
            generation: tasks.len(),
            manager,
            task,
            name: String::from(name),
            spawner,
            polls: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            wall: AtomicU64::new(0),
            max_wall: AtomicU64::new(0),
            clock_errors: AtomicU64::new(0),
            completed: AtomicBool::new(false),
            poll_active: AtomicBool::new(false),
            allocations: AtomicU64::new(0),
            allocated_bytes: AtomicU64::new(0),
            thread_id: unsafe { libc::syscall(libc::SYS_gettid) as u64 },
        });
        tasks.push(counters.clone());
        counters
    };
    let mut future = pin!(future);
    poll_fn(|context| {
        let _current =
            CurrentGuard(CURRENT.with(|current| current.replace(Some(counters.clone()))));
        let cpu_start = thread_cpu_nanoseconds();
        let started = Instant::now();
        counters.poll_active.store(true, Ordering::Relaxed);
        let active = ActivePoll(&counters.poll_active);
        let result = future.as_mut().poll(context);
        drop(active);
        let elapsed = started.elapsed().as_nanos() as u64;
        let cpu_end = thread_cpu_nanoseconds();
        counters.polls.fetch_add(1, Ordering::Relaxed);
        counters.wall.fetch_add(elapsed, Ordering::Relaxed);
        counters.max_wall.fetch_max(elapsed, Ordering::Relaxed);
        if let (Some(start), Some(end)) = (cpu_start, cpu_end) {
            counters
                .cpu
                .fetch_add(end.saturating_sub(start), Ordering::Relaxed);
        } else {
            counters.clock_errors.fetch_add(1, Ordering::Relaxed);
        }
        if result.is_ready() {
            counters.completed.store(true, Ordering::Relaxed);
        }
        result
    })
    .await
}

/// Copy cumulative counters; concurrent polling can advance during the snapshot.
pub fn snapshot() -> Vec<TaskSample> {
    TASKS
        .lock()
        .unwrap()
        .iter()
        .map(|c| TaskSample {
            generation: c.generation,
            manager: c.manager,
            task: c.task,
            name: c.name.clone(),
            spawner: c.spawner,
            polls: c.polls.load(Ordering::Relaxed),
            cpu_nanoseconds: c.cpu.load(Ordering::Relaxed),
            poll_wall_nanoseconds: c.wall.load(Ordering::Relaxed),
            max_poll_wall_nanoseconds: c.max_wall.load(Ordering::Relaxed),
            clock_errors: c.clock_errors.load(Ordering::Relaxed),
            completed: c.completed.load(Ordering::Relaxed),
            poll_active: c.poll_active.load(Ordering::Relaxed),
            allocation_requests: c.allocations.load(Ordering::Relaxed),
            allocation_request_bytes: c.allocated_bytes.load(Ordering::Relaxed),
            thread_id: c.thread_id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn pending_time_is_not_charged_to_task_polling() {
        let mut count = 0;
        let future = core::future::poll_fn(move |_| {
            count += 1;
            if count == 1 {
                Poll::Pending
            } else {
                Poll::Ready(42)
            }
        });
        let mut future = pin!(instrument(99, 7, "profiling-pending-test", 3, future));
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        std::thread::sleep(core::time::Duration::from_millis(50));
        assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(42));
        let sample = snapshot()
            .into_iter()
            .find(|s| s.name == "profiling-pending-test")
            .unwrap();
        assert_eq!(sample.polls, 2);
        assert!(sample.completed);
        assert_eq!(sample.spawner, 3);
        assert!(sample.poll_wall_nanoseconds < 50_000_000);
    }

    #[test]
    fn a_running_poll_is_visible_before_it_returns() {
        embassy_futures::block_on(instrument(95, 1, "active-poll-test", 0, async {
            let samples = snapshot();
            assert!(
                samples
                    .iter()
                    .find(|s| s.name == "active-poll-test")
                    .unwrap()
                    .poll_active
            );
        }));
        let samples = snapshot();
        assert!(
            !samples
                .iter()
                .find(|s| s.name == "active-poll-test")
                .unwrap()
                .poll_active
        );
    }

    #[test]
    fn attribution_during_thread_local_teardown_does_not_panic() {
        struct LateDrop;
        impl Drop for LateDrop {
            fn drop(&mut self) {
                record_allocation(1);
            }
        }
        std::thread_local! { static LATE_DROP: LateDrop = const { LateDrop }; }
        std::thread::spawn(|| {
            LATE_DROP.with(|_| {});
            embassy_futures::block_on(instrument(96, 1, "tls-teardown", 0, async {}));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn allocation_volume_is_attributed_only_during_polls() {
        record_allocation(500);
        embassy_futures::block_on(instrument(97, 3, "allocation-attribution", 0, async {
            record_allocation(37);
            record_allocation(79);
        }));
        record_allocation(500);
        let sample = snapshot()
            .into_iter()
            .find(|s| s.name == "allocation-attribution")
            .unwrap();
        assert_eq!(sample.allocation_requests, 2);
        assert_eq!(sample.allocation_request_bytes, 116);
    }

    #[test]
    fn reused_task_ids_keep_separate_generations() {
        embassy_futures::block_on(instrument(98, 5, "first-generation", 0, async {}));
        embassy_futures::block_on(instrument(98, 5, "second-generation", 0, async {}));
        let samples = snapshot();
        let first = samples
            .iter()
            .find(|s| s.name == "first-generation")
            .unwrap();
        let second = samples
            .iter()
            .find(|s| s.name == "second-generation")
            .unwrap();
        assert_ne!(first.generation, second.generation);
        assert_eq!(first.polls, 1);
        assert_eq!(second.polls, 1);
    }
}
