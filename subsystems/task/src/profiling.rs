//! Optional Linux task poll accounting. CPU time excludes time between polls.

use alloc::{collections::VecDeque, format, string::String, sync::Arc, vec::Vec};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::{sync::Mutex, time::Instant};

std::thread_local! { static CURRENT: core::cell::RefCell<Option<Arc<Counters>>> = const { core::cell::RefCell::new(None) }; }

/// Finished tasks kept individually; older ones are folded into one summary.
pub const FINISHED_HISTORY: usize = 64;

static TASKS: Mutex<Registry> = Mutex::new(Registry {
    next_generation: 0,
    live: Vec::new(),
    finished: VecDeque::new(),
    retired: Retired {
        tasks: 0,
        polls: 0,
        cpu: 0,
        wall: 0,
        max_wall: 0,
        clock_errors: 0,
        completed: 0,
        allocations: 0,
        allocated_bytes: 0,
    },
});

/// Bounded: live tasks, the last [`FINISHED_HISTORY`] finished ones and a
/// summary of all older finished tasks.
struct Registry {
    next_generation: usize,
    live: Vec<Arc<Counters>>,
    finished: VecDeque<Arc<Counters>>,
    retired: Retired,
}

/// Totals of finished tasks evicted from the history, so cumulative reports
/// keep their CPU, wall-clock and allocation totals.
struct Retired {
    tasks: u64,
    polls: u64,
    cpu: u64,
    wall: u64,
    max_wall: u64,
    clock_errors: u64,
    completed: u64,
    allocations: u64,
    allocated_bytes: u64,
}

impl Counters {
    fn sample(&self) -> TaskSample {
        TaskSample {
            generation: self.generation,
            manager: self.manager,
            task: self.task,
            name: self.name.clone(),
            spawner: self.spawner,
            polls: self.polls.load(Ordering::Relaxed),
            cpu_nanoseconds: self.cpu.load(Ordering::Relaxed),
            poll_wall_nanoseconds: self.wall.load(Ordering::Relaxed),
            max_poll_wall_nanoseconds: self.max_wall.load(Ordering::Relaxed),
            clock_errors: self.clock_errors.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            poll_active: self.poll_active.load(Ordering::Relaxed),
            allocation_requests: self.allocations.load(Ordering::Relaxed),
            allocation_request_bytes: self.allocated_bytes.load(Ordering::Relaxed),
            thread_id: self.thread_id,
        }
    }
}

impl Registry {
    fn finish(&mut self, counters: &Arc<Counters>) {
        let Some(position) = self.live.iter().position(|c| Arc::ptr_eq(c, counters)) else {
            return;
        };
        let counters = self.live.swap_remove(position);
        if self.finished.len() == FINISHED_HISTORY
            && let Some(oldest) = self.finished.pop_front()
        {
            self.retire(&oldest);
        }
        self.finished.push_back(counters);
    }

    fn retire(&mut self, counters: &Counters) {
        let retired = &mut self.retired;
        retired.tasks += 1;
        retired.polls += counters.polls.load(Ordering::Relaxed);
        retired.cpu += counters.cpu.load(Ordering::Relaxed);
        retired.wall += counters.wall.load(Ordering::Relaxed);
        retired.max_wall = retired
            .max_wall
            .max(counters.max_wall.load(Ordering::Relaxed));
        retired.clock_errors += counters.clock_errors.load(Ordering::Relaxed);
        retired.completed += u64::from(counters.completed.load(Ordering::Relaxed));
        retired.allocations += counters.allocations.load(Ordering::Relaxed);
        retired.allocated_bytes += counters.allocated_bytes.load(Ordering::Relaxed);
    }
}

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

/// A cumulative sample of a live task, a recently finished one, or (with
/// [`RETIRED_GENERATION`]) the summed totals of older finished tasks.
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

/// Generation of the summary sample; its `task` field counts the tasks folded
/// into it and `completed` is true only if all of them completed.
pub const RETIRED_GENERATION: usize = usize::MAX;

/// Moves the task out of the live list when its future completes or is dropped.
struct Finish(Arc<Counters>);
impl Drop for Finish {
    fn drop(&mut self) {
        if let Ok(mut tasks) = TASKS.lock() {
            tasks.finish(&self.0);
        }
    }
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
            generation: tasks.next_generation,
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
        tasks.next_generation += 1;
        tasks.live.push(counters.clone());
        counters
    };
    let finish = Finish(counters.clone());
    let mut future = pin!(future);
    let output = poll_fn(|context| {
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
    .await;
    drop(finish);
    output
}

/// Copy cumulative counters; concurrent polling can advance during the snapshot.
pub fn snapshot() -> Vec<TaskSample> {
    let tasks = TASKS.lock().unwrap();
    let mut samples: Vec<TaskSample> = tasks
        .live
        .iter()
        .chain(tasks.finished.iter())
        .map(|c| c.sample())
        .collect();
    let retired = &tasks.retired;
    if retired.tasks > 0 {
        samples.push(TaskSample {
            generation: RETIRED_GENERATION,
            manager: 0,
            task: retired.tasks as usize,
            name: format!("(retired: {} tasks)", retired.tasks),
            spawner: 0,
            polls: retired.polls,
            cpu_nanoseconds: retired.cpu,
            poll_wall_nanoseconds: retired.wall,
            max_poll_wall_nanoseconds: retired.max_wall,
            clock_errors: retired.clock_errors,
            completed: retired.completed == retired.tasks,
            poll_active: false,
            allocation_requests: retired.allocations,
            allocation_request_bytes: retired.allocated_bytes,
            thread_id: 0,
        });
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    /// The counters of the task being polled. Other tests finish tasks in
    /// parallel and can evict a finished task from the history, so tests read
    /// their own task's counters instead of searching a later snapshot.
    fn current() -> Arc<Counters> {
        CURRENT.with(|current| current.borrow().clone()).unwrap()
    }

    fn is_live(counters: &Arc<Counters>) -> bool {
        TASKS
            .lock()
            .unwrap()
            .live
            .iter()
            .any(|c| Arc::ptr_eq(c, counters))
    }

    #[test]
    fn pending_time_is_not_charged_to_task_polling() {
        let mut count = 0;
        let counters = core::cell::RefCell::new(None);
        let future = core::future::poll_fn(|_| {
            counters.borrow_mut().get_or_insert_with(current);
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
        let sample = counters.borrow().as_ref().unwrap().sample();
        assert_eq!(sample.polls, 2);
        assert!(sample.completed);
        assert_eq!(sample.spawner, 3);
        assert!(sample.poll_wall_nanoseconds < 50_000_000);
    }

    #[test]
    fn a_running_poll_is_visible_before_it_returns() {
        let counters = embassy_futures::block_on(instrument(95, 1, "active-poll-test", 0, async {
            let counters = current();
            assert!(
                snapshot()
                    .iter()
                    .any(|s| s.generation == counters.generation && s.poll_active)
            );
            counters
        }));
        assert!(!counters.sample().poll_active);
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
        let counters =
            embassy_futures::block_on(instrument(97, 3, "allocation-attribution", 0, async {
                record_allocation(37);
                record_allocation(79);
                current()
            }));
        record_allocation(500);
        let sample = counters.sample();
        assert_eq!(sample.allocation_requests, 2);
        assert_eq!(sample.allocation_request_bytes, 116);
    }

    #[test]
    fn reused_task_ids_keep_separate_generations() {
        let first = embassy_futures::block_on(instrument(98, 5, "first-generation", 0, async {
            current()
        }));
        let second = embassy_futures::block_on(instrument(98, 5, "second-generation", 0, async {
            current()
        }));
        assert_ne!(first.generation, second.generation);
        assert_eq!(first.sample().polls, 1);
        assert_eq!(second.sample().polls, 1);
    }

    #[test]
    fn finished_history_is_bounded_and_retired_totals_are_kept() {
        for _ in 0..FINISHED_HISTORY + 8 {
            embassy_futures::block_on(instrument(94, 1, "history-flood", 0, async {}));
        }
        let samples = snapshot();
        let tasks = TASKS.lock().unwrap();
        assert!(tasks.finished.len() <= FINISHED_HISTORY);
        assert!(tasks.retired.tasks >= 8);
        drop(tasks);
        let retired = samples
            .iter()
            .find(|s| s.generation == RETIRED_GENERATION)
            .unwrap();
        assert!(retired.task >= 8);
        assert!(retired.polls >= 8);
        assert!(!retired.poll_active);
    }

    #[test]
    fn a_dropped_task_leaves_the_live_list() {
        let mut counters = None;
        let future = core::future::poll_fn(|_| {
            counters.get_or_insert_with(current);
            Poll::<()>::Pending
        });
        {
            let mut future = pin!(instrument(93, 2, "dropped-task", 0, future));
            let mut context = Context::from_waker(Waker::noop());
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
        let counters = counters.unwrap();
        assert!(!is_live(&counters));
        assert!(!counters.sample().completed);
    }
}
