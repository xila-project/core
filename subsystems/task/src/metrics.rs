//! Portable CPU activity metrics for tasks and executors (`metrics` feature).
//!
//! Every poll of a managed task is timed with Embassy's monotonic clock, which
//! each platform already provides for `sleep`. The clock is read twice per
//! poll; nothing is allocated on the poll path. Per-task counters live beside
//! the task storage slots and per-executor counters are created once per
//! executor, so spawning allocates nothing more either.
//!
//! All counters only grow. Consumers sample [`Manager::get_metrics`](crate::Manager::get_metrics) twice and
//! divide differences, for example `Δbusy / Δtimestamp` for an executor's
//! CPU share over that window. Busy time is attributed when a poll ends, and
//! an executor also reports its poll or idle period in progress, so its busy
//! and idle totals stay continuous on multi-core and multi-thread platforms.
//!
//! Durations are rounded to the clock's ticks (`embassy_time::TICK_HZ`). With
//! a coarse clock, such as a 32,768 Hz real-time counter, each poll is
//! measured as a whole number of ticks; as polls start at uncorrelated
//! phases, totals over many polls remain unbiased.
//!
//! Without the feature, [`Manager::get_metrics`](crate::Manager::get_metrics) returns `None` and
//! [`ExecutorActivity::measure_idle`] only runs its closure, so platforms and
//! applications can use the API unconditionally at no cost.
//!
//! # Platform hook
//!
//! Executor busy time is derived from task polls and needs no platform code.
//! Idle time, the time an executor waits for work, can only be measured by the
//! executor loop. Platforms wrap their wait with [`ExecutorActivity::measure_idle`]:
//!
//! ```ignore
//! let activity = task::metrics::ExecutorActivity::get(spawner.executor_id());
//! loop {
//!     unsafe { executor.poll() };
//!     activity.measure_idle(|| cortex_m::asm::wfe());
//! }
//! ```
//!
//! Interrupt handlers that run during a task poll count as that task's busy
//! time; those that run while the executor waits count as idle time. Without
//! the hook, [`ExecutorMetrics::idle_nanoseconds`] is `None`.
//!
//! Counters are read with a sequence lock written only by the executor that
//! owns them. Read them from tasks, never from an interrupt handler that can
//! preempt an instrumented executor.

use crate::{SpawnerIdentifier, TaskIdentifier};
use alloc::{string::String, vec::Vec};

/// Cumulative counters of one registered task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMetrics {
    pub identifier: TaskIdentifier,
    /// Distinguishes registrations that reuse the same identifier.
    pub generation: u64,
    pub name: String,
    pub spawner: SpawnerIdentifier,
    pub polls: u64,
    pub busy_nanoseconds: u64,
    pub longest_poll_nanoseconds: u64,
}

/// Cumulative counters of one executor (a core or an operating system thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutorMetrics {
    /// Lowest spawner registration of this executor.
    pub spawner: SpawnerIdentifier,
    /// Embassy executor identifier (`Spawner::executor_id`).
    pub executor: usize,
    pub polls: u64,
    /// Time spent polling managed tasks, including a poll in progress.
    pub busy_nanoseconds: u64,
    /// Time spent waiting for work, including a wait in progress, when the
    /// platform executor reports it through [`ExecutorActivity::measure_idle`].
    pub idle_nanoseconds: Option<u64>,
    /// Clock value when the counters were created.
    pub started_nanoseconds: u64,
}

/// Sample of all task and executor counters. Each task's and each executor's
/// counters are read consistently; the set is not one atomic instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// Monotonic clock value of the sample, on the same clock as the counters.
    pub timestamp_nanoseconds: u64,
    pub tasks: Vec<TaskMetrics>,
    pub executors: Vec<ExecutorMetrics>,
}

/// True when the `metrics` feature instruments task polls.
pub const ENABLED: bool = cfg!(feature = "metrics");

#[cfg(feature = "metrics")]
pub use enabled::*;

#[cfg(not(feature = "metrics"))]
pub use disabled::*;

#[cfg(not(feature = "metrics"))]
mod disabled {
    /// Executor activity hook; does nothing without the `metrics` feature.
    pub struct ExecutorActivity;

    static DISABLED: ExecutorActivity = ExecutorActivity;

    impl ExecutorActivity {
        #[inline(always)]
        pub fn get(_executor: usize) -> &'static Self {
            &DISABLED
        }

        #[inline(always)]
        pub fn measure_idle<R>(&self, wait: impl FnOnce() -> R) -> R {
            wait()
        }
    }
}

#[cfg(feature = "metrics")]
mod enabled {
    use super::ExecutorMetrics;
    use alloc::{boxed::Box, vec::Vec};
    use core::cell::RefCell;
    use portable_atomic::{AtomicU32, Ordering, fence};
    use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

    /// Current clock value in ticks.
    #[inline(always)]
    pub(crate) fn now() -> u64 {
        embassy_time::Instant::now().as_ticks()
    }

    /// Convert clock ticks to nanoseconds, saturating.
    pub fn ticks_to_nanoseconds(ticks: u64) -> u64 {
        const HERTZ: u64 = embassy_time::TICK_HZ;
        if 1_000_000_000 % HERTZ == 0 {
            ticks.saturating_mul(1_000_000_000 / HERTZ)
        } else {
            let nanoseconds = u128::from(ticks) * 1_000_000_000 / u128::from(HERTZ);
            u64::try_from(nanoseconds).unwrap_or(u64::MAX)
        }
    }

    /// `N` words updated together by one writer and read consistently by any
    /// thread. Only atomic loads and stores are used, so this also works on
    /// targets without compare-and-swap or 64-bit atomics.
    pub(crate) struct Words<const N: usize> {
        sequence: AtomicU32,
        words: [AtomicU32; N],
    }

    impl<const N: usize> Words<N> {
        pub(crate) const fn new() -> Self {
            Self {
                sequence: AtomicU32::new(0),
                words: [const { AtomicU32::new(0) }; N],
            }
        }

        /// Run `update`, which loads and stores words with [`Self::get`] and
        /// [`Self::set`], as one atomic change for readers. Only one thread
        /// may write at a time; ownership changes need a happens-before edge,
        /// such as Embassy's task claim.
        #[inline(always)]
        pub(crate) fn write(&self, update: impl FnOnce(&Self)) {
            let sequence = self.sequence.load(Ordering::Relaxed);
            self.sequence
                .store(sequence.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            update(self);
            self.sequence
                .store(sequence.wrapping_add(2), Ordering::Release);
        }

        /// Writer-side load; readers use [`Self::read`].
        #[inline(always)]
        pub(crate) fn get(&self, index: usize) -> u32 {
            self.words[index].load(Ordering::Relaxed)
        }

        #[inline(always)]
        pub(crate) fn set(&self, index: usize, value: u32) {
            self.words[index].store(value, Ordering::Relaxed);
        }

        #[inline(always)]
        fn get64(&self, index: usize) -> u64 {
            u64::from(self.get(index)) | (u64::from(self.get(index + 1)) << 32)
        }

        #[inline(always)]
        fn set64(&self, index: usize, value: u64) {
            self.set(index, value as u32);
            self.set(index + 1, (value >> 32) as u32);
        }

        #[inline(always)]
        fn add64(&self, index: usize, value: u64) {
            self.set64(index, self.get64(index).wrapping_add(value));
        }

        pub(crate) fn read(&self) -> [u32; N] {
            loop {
                let before = self.sequence.load(Ordering::Acquire);
                if before & 1 == 0 {
                    let values =
                        core::array::from_fn(|index| self.words[index].load(Ordering::Relaxed));
                    fence(Ordering::Acquire);
                    if self.sequence.load(Ordering::Relaxed) == before {
                        return values;
                    }
                }
                core::hint::spin_loop();
            }
        }
    }

    fn get64(words: &[u32], index: usize) -> u64 {
        u64::from(words[index]) | (u64::from(words[index + 1]) << 32)
    }

    const TASK_BUSY: usize = 0;
    const TASK_POLLS: usize = 2;
    const TASK_LONGEST: usize = 4;

    /// Per-slot task counters, in ticks. Retained with their storage slot and
    /// reset when the slot is claimed by a new task.
    pub(crate) struct TaskCounters(Words<5>);

    /// Task counters in ticks.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct TaskSample {
        pub(crate) polls: u64,
        pub(crate) busy: u64,
        pub(crate) longest_poll: u64,
    }

    impl TaskCounters {
        pub(crate) const fn new() -> Self {
            Self(Words::new())
        }

        pub(crate) fn reset(&self) {
            self.0
                .write(|words| (0..5).for_each(|index| words.set(index, 0)));
        }

        #[inline(always)]
        pub(crate) fn record_poll(&self, elapsed: u64) {
            self.0.write(|words| {
                words.add64(TASK_BUSY, elapsed);
                words.add64(TASK_POLLS, 1);
                let elapsed = u32::try_from(elapsed).unwrap_or(u32::MAX);
                if elapsed > words.get(TASK_LONGEST) {
                    words.set(TASK_LONGEST, elapsed);
                }
            });
        }

        pub(crate) fn sample(&self) -> TaskSample {
            let words = self.0.read();
            TaskSample {
                polls: get64(&words, TASK_POLLS),
                busy: get64(&words, TASK_BUSY),
                longest_poll: u64::from(words[TASK_LONGEST]),
            }
        }
    }

    const EXECUTOR_BUSY: usize = 0;
    const EXECUTOR_IDLE: usize = 2;
    const EXECUTOR_POLLS: usize = 4;
    const EXECUTOR_SINCE: usize = 6;
    const EXECUTOR_STATE: usize = 8;

    /// Executor state in the low bits; zero between polls and waits.
    const STATE_MASK: u32 = 0b11;
    const STATE_BUSY: u32 = 1;
    const STATE_IDLE: u32 = 2;
    /// Set once the platform reported an idle period.
    const IDLE_MEASURED: u32 = 0b100;

    /// Busy and idle counters of one executor, shared by its spawner
    /// registrations. Platforms use [`ExecutorActivity::measure_idle`].
    pub struct ExecutorActivity {
        executor: usize,
        started: u64,
        words: Words<9>,
    }

    static EXECUTORS: Mutex<CriticalSectionRawMutex, RefCell<Vec<&'static ExecutorActivity>>> =
        Mutex::new(RefCell::new(Vec::new()));

    impl ExecutorActivity {
        pub(crate) const fn new(executor: usize, started: u64) -> Self {
            Self {
                executor,
                started,
                words: Words::new(),
            }
        }

        /// Counters of the executor `executor` (`Spawner::executor_id`),
        /// created on first use and kept for the program's lifetime.
        pub fn get(executor: usize) -> &'static Self {
            EXECUTORS.lock(|executors| {
                let mut executors = executors.borrow_mut();
                if let Some(activity) = executors.iter().find(|a| a.executor == executor) {
                    return *activity;
                }
                let activity: &'static Self = Box::leak(Box::new(Self::new(executor, now())));
                executors.push(activity);
                activity
            })
        }

        pub fn executor(&self) -> usize {
            self.executor
        }

        /// Run the executor's wait for work, counting its duration as idle.
        /// Call it only from the executor's own thread.
        #[inline]
        pub fn measure_idle<R>(&self, wait: impl FnOnce() -> R) -> R {
            self.begin(STATE_IDLE, now());
            let result = wait();
            self.end(now());
            result
        }

        #[inline(always)]
        pub(crate) fn begin_poll(&self, at: u64) {
            self.begin(STATE_BUSY, at);
        }

        #[inline(always)]
        fn begin(&self, state: u32, at: u64) {
            self.words.write(|words| {
                words.set64(EXECUTOR_SINCE, at);
                let mut flags = words.get(EXECUTOR_STATE) & IDLE_MEASURED;
                if state == STATE_IDLE {
                    flags |= IDLE_MEASURED;
                }
                words.set(EXECUTOR_STATE, state | flags);
            });
        }

        #[inline(always)]
        pub(crate) fn end(&self, at: u64) {
            self.words.write(|words| {
                let elapsed = at.saturating_sub(words.get64(EXECUTOR_SINCE));
                let state = words.get(EXECUTOR_STATE);
                match state & STATE_MASK {
                    STATE_BUSY => {
                        words.add64(EXECUTOR_BUSY, elapsed);
                        words.add64(EXECUTOR_POLLS, 1);
                    }
                    STATE_IDLE => words.add64(EXECUTOR_IDLE, elapsed),
                    _ => {}
                }
                words.set(EXECUTOR_STATE, state & !STATE_MASK);
            });
        }

        /// Counters at `at`, including the period in progress.
        pub(crate) fn sample(&self, spawner: usize, at: u64) -> ExecutorMetrics {
            let words = self.words.read();
            let mut busy = get64(&words, EXECUTOR_BUSY);
            let mut idle = get64(&words, EXECUTOR_IDLE);
            let current = at.saturating_sub(get64(&words, EXECUTOR_SINCE));
            match words[EXECUTOR_STATE] & STATE_MASK {
                STATE_BUSY => busy = busy.wrapping_add(current),
                STATE_IDLE => idle = idle.wrapping_add(current),
                _ => {}
            }
            ExecutorMetrics {
                spawner,
                executor: self.executor,
                polls: get64(&words, EXECUTOR_POLLS),
                busy_nanoseconds: ticks_to_nanoseconds(busy),
                idle_nanoseconds: (words[EXECUTOR_STATE] & IDLE_MEASURED != 0)
                    .then(|| ticks_to_nanoseconds(idle)),
                started_nanoseconds: ticks_to_nanoseconds(self.started),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn task_counters_accumulate_with_a_test_clock() {
            let counters = TaskCounters::new();
            counters.record_poll(3);
            counters.record_poll(10);
            counters.record_poll(0);
            assert_eq!(
                counters.sample(),
                TaskSample {
                    polls: 3,
                    busy: 13,
                    longest_poll: 10
                }
            );
            counters.reset();
            assert_eq!(counters.sample(), TaskSample::default());
        }

        #[test]
        fn task_counters_carry_into_the_high_word() {
            let counters = TaskCounters::new();
            counters.record_poll(u64::from(u32::MAX));
            counters.record_poll(2);
            counters.record_poll(u64::MAX / 4);
            let sample = counters.sample();
            assert_eq!(sample.busy, u64::from(u32::MAX) + 2 + u64::MAX / 4);
            assert_eq!(sample.longest_poll, u64::from(u32::MAX));
        }

        #[test]
        fn executor_busy_and_idle_with_a_test_clock() {
            let activity = ExecutorActivity::new(1, 100);
            let at = |ticks| activity.sample(7, ticks);
            assert_eq!(at(100).busy_nanoseconds, 0);
            assert_eq!(at(100).idle_nanoseconds, None);

            activity.begin(STATE_BUSY, 110);
            // A poll in progress is already visible.
            assert_eq!(at(115).busy_nanoseconds, ticks_to_nanoseconds(5));
            activity.end(120);
            activity.begin(STATE_IDLE, 120);
            assert_eq!(at(150).idle_nanoseconds, Some(ticks_to_nanoseconds(30)));
            activity.end(200);
            activity.begin(STATE_BUSY, 205);
            activity.end(206);

            let sample = at(1000);
            assert_eq!(sample.polls, 2);
            assert_eq!(sample.spawner, 7);
            assert_eq!(sample.executor, 1);
            assert_eq!(sample.busy_nanoseconds, ticks_to_nanoseconds(11));
            assert_eq!(sample.idle_nanoseconds, Some(ticks_to_nanoseconds(80)));
            assert_eq!(sample.started_nanoseconds, ticks_to_nanoseconds(100));
            // Idle stays reported once measured, and counters only grow.
            assert!(at(2000).idle_nanoseconds.is_some());
            assert_eq!(at(2000), sample);
        }

        #[test]
        fn executor_counters_are_shared_per_executor() {
            let first = ExecutorActivity::get(usize::MAX - 1);
            let second = ExecutorActivity::get(usize::MAX - 1);
            let other = ExecutorActivity::get(usize::MAX - 2);
            assert!(core::ptr::eq(first, second));
            assert!(!core::ptr::eq(first, other));
            assert_eq!(first.executor(), usize::MAX - 1);
        }

        #[test]
        fn measured_idle_runs_the_wait() {
            let activity = ExecutorActivity::get(usize::MAX - 3);
            assert_eq!(activity.measure_idle(|| 42), 42);
            assert!(activity.sample(0, now()).idle_nanoseconds.is_some());
        }

        #[test]
        fn tick_conversion_saturates() {
            assert_eq!(ticks_to_nanoseconds(0), 0);
            assert_eq!(ticks_to_nanoseconds(embassy_time::TICK_HZ), 1_000_000_000);
            assert_eq!(ticks_to_nanoseconds(u64::MAX), u64::MAX);
        }

        #[test]
        fn readers_never_observe_torn_counters() {
            extern crate std;
            use portable_atomic::AtomicBool;
            static COUNTERS: TaskCounters = TaskCounters::new();
            static DONE: AtomicBool = AtomicBool::new(false);
            // Steps cross the 32-bit boundary often, so a torn read would
            // break the busy = polls * step invariant.
            const STEP: u64 = (1 << 31) + 1;
            let writer = std::thread::spawn(|| {
                for _ in 0..200_000 {
                    COUNTERS.record_poll(STEP);
                }
                DONE.store(true, Ordering::Release);
            });
            let mut previous = 0;
            while !DONE.load(Ordering::Acquire) {
                let sample = COUNTERS.sample();
                assert_eq!(sample.busy, sample.polls * STEP);
                assert!(sample.polls >= previous);
                previous = sample.polls;
            }
            writer.join().unwrap();
            assert_eq!(COUNTERS.sample().polls, 200_000);
        }
    }
}

#[cfg(all(test, not(feature = "metrics")))]
mod tests {
    use super::*;

    #[test]
    fn disabled_hook_only_runs_the_wait() {
        const { assert!(!ENABLED) };
        assert_eq!(core::mem::size_of::<ExecutorActivity>(), 0);
        assert_eq!(ExecutorActivity::get(1).measure_idle(|| 42), 42);
    }
}
