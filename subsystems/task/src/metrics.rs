//! Portable CPU activity metrics for tasks and executors (`metrics` feature).
//!
//! Every poll of a managed task is timed with a monotonic clock: the
//! operating system's monotonic clock on Linux, macOS and Windows, and
//! Embassy's clock, which each platform already provides for `sleep`,
//! everywhere else. The clock is read twice per poll; nothing is allocated on
//! the poll path. Per-task counters live beside the task storage slots and
//! per-executor counters are created once per executor, so spawning allocates
//! nothing more either.
//!
//! # What is measured
//!
//! Busy time is wall-clock time spent inside task polls, not processor time:
//! on hosts, time the thread is descheduled or blocked during a poll counts
//! as busy. Durations are rounded to the clock's resolution
//! (`TICK_HERTZ`). With a coarse clock, such as a 32,768 Hz real-time
//! counter, each poll is measured as a whole number of ticks; as polls start
//! at uncorrelated phases, totals over many polls remain unbiased.
//!
//! Interrupt handlers that run during a task poll count as that task's busy
//! time; those that run while the executor waits count as idle time.
//!
//! # Computing loads
//!
//! All counters only grow. Consumers sample
//! [`Manager::get_metrics`](crate::Manager::get_metrics) periodically and
//! divide differences between two snapshots:
//!
//! - Match executors between snapshots by [`ExecutorMetrics::executor`], not
//!   by [`ExecutorMetrics::spawner`]: an executor is reported under its lowest
//!   spawner registration, which can change.
//! - An executor's load over the window is `Δbusy / Δtimestamp` with that
//!   executor's own [`ExecutorMetrics::timestamp_nanoseconds`]. When
//!   [`ExecutorMetrics::idle_nanoseconds`] is available on both sides, prefer
//!   `1 − Δidle / Δtimestamp`: it also counts executor bookkeeping and
//!   foreign work on that core as load. Both include the poll or wait in
//!   progress, so they are continuous and stay within `[0, 1]`.
//! - A task's share is `Δbusy / Δ`[`MetricsSnapshot::timestamp_nanoseconds`].
//!   Task busy time is added when a poll ends, so a long poll counts entirely
//!   in the window where it ends: clamp per-task shares to `[0, 1]`. Compare
//!   [`TaskMetrics::generation`] to detect a reused task identifier.
//! - Tasks adopted rather than spawned by the manager have no counters, report
//!   zero polls and have `spawner == usize::MAX`.
//!
//! Without the feature, [`Manager::get_metrics`](crate::Manager::get_metrics)
//! returns `None` and [`ExecutorActivity::measure_idle`] only runs its
//! closure, so platforms and applications can use the API unconditionally at
//! no cost.
//!
//! # Platform hook
//!
//! Executor busy time is derived from task polls and needs no platform code.
//! Idle time, the time an executor waits for work, can only be measured by the
//! executor loop. Platforms wrap their wait with
//! [`ExecutorActivity::measure_idle`], between polls of their executor:
//!
//! ```ignore
//! let activity = task::metrics::ExecutorActivity::new(executor.spawner());
//! loop {
//!     unsafe { executor.poll() };
//!     activity.measure_idle(|| cortex_m::asm::wfe());
//! }
//! ```
//!
//! Without the hook, [`ExecutorMetrics::idle_nanoseconds`] is `None`.
//!
//! # Concurrency
//!
//! Each executor's counters have a single writer, the executor's own thread:
//! task polls and [`ExecutorActivity::measure_idle`], whose handle is created
//! from that executor's `!Send` [`Spawner`](embassy_executor::Spawner) and
//! cannot leave its thread.
//! Readers use a sequence lock built from 32-bit atomic loads and stores, so
//! it works without compare-and-swap or 64-bit atomics, and they spin while a
//! write is in progress. Never read the counters from code that preempts a
//! writer on the same core: an interrupt handler, or a task of an interrupt
//! executor that preempts an instrumented thread executor. Executor counters
//! are read inside the manager's critical section, so such a reader would spin
//! forever with interrupts disabled.

use crate::{SpawnerIdentifier, TaskIdentifier};
use alloc::{string::String, vec::Vec};

/// Cumulative counters of one registered task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMetrics {
    pub identifier: TaskIdentifier,
    /// Distinguishes registrations that reuse the same identifier.
    pub generation: u64,
    pub name: String,
    /// Spawner registration running the task; `usize::MAX` for adopted tasks.
    pub spawner: SpawnerIdentifier,
    pub polls: u64,
    /// Wall-clock time spent in completed polls.
    pub busy_nanoseconds: u64,
    pub longest_poll_nanoseconds: u64,
}

/// Cumulative counters of one executor (a core or an operating system thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutorMetrics {
    /// Lowest spawner registration of this executor. It can change between
    /// snapshots; match executors by [`Self::executor`].
    pub spawner: SpawnerIdentifier,
    /// Embassy executor identifier (`Spawner::executor_id`), stable for the
    /// executor's lifetime.
    pub executor: usize,
    pub polls: u64,
    /// Wall-clock time spent polling managed tasks, including a poll in
    /// progress.
    pub busy_nanoseconds: u64,
    /// Time spent waiting for work, including a wait in progress, when the
    /// platform executor reports it through [`ExecutorActivity::measure_idle`].
    pub idle_nanoseconds: Option<u64>,
    /// Clock value when the counters were created.
    pub started_nanoseconds: u64,
    /// Clock value at which these counters were read. Divide busy and idle
    /// differences by differences of this value.
    pub timestamp_nanoseconds: u64,
}

/// Sample of all task and executor counters. Each task's and each executor's
/// counters are read consistently; the set is not one atomic instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// Monotonic clock value read after every counter of the snapshot, on the
    /// same clock as the counters. Use it for task shares; executors carry
    /// their own [`ExecutorMetrics::timestamp_nanoseconds`].
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
    use core::marker::PhantomData;
    use embassy_executor::Spawner;

    /// Executor idle hook; does nothing without the `metrics` feature.
    pub struct ExecutorActivity {
        not_send: PhantomData<*mut ()>,
    }

    impl ExecutorActivity {
        #[inline(always)]
        pub fn new(_spawner: Spawner) -> Self {
            Self {
                not_send: PhantomData,
            }
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
    use core::{cell::RefCell, marker::PhantomData};
    use embassy_executor::Spawner;
    use portable_atomic::{AtomicU32, Ordering, fence};
    use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    mod clock {
        extern crate std;
        use std::{sync::OnceLock, time::Instant};

        /// The operating system's monotonic clock, in nanoseconds.
        pub const HERTZ: u64 = 1_000_000_000;

        /// Nanoseconds since the first read. Embassy's std clock takes its
        /// driver's global mutex on every read; this one is lock-free.
        #[inline(always)]
        pub fn now() -> u64 {
            static ORIGIN: OnceLock<Instant> = OnceLock::new();
            let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_nanos();
            u64::try_from(elapsed).unwrap_or(u64::MAX)
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    mod clock {
        /// Embassy's tick rate, set by the platform's time driver.
        pub const HERTZ: u64 = embassy_time::TICK_HZ;

        #[inline(always)]
        pub fn now() -> u64 {
            embassy_time::Instant::now().as_ticks()
        }
    }

    /// Resolution of the metrics clock, in ticks per second.
    pub const TICK_HERTZ: u64 = clock::HERTZ;

    /// Current clock value in ticks.
    #[inline(always)]
    pub(crate) fn now() -> u64 {
        clock::now()
    }

    /// Convert clock ticks to nanoseconds, saturating.
    pub fn ticks_to_nanoseconds(ticks: u64) -> u64 {
        if 1_000_000_000 % TICK_HERTZ == 0 {
            ticks.saturating_mul(1_000_000_000 / TICK_HERTZ)
        } else {
            let nanoseconds = u128::from(ticks) * 1_000_000_000 / u128::from(TICK_HERTZ);
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

        /// Like [`Self::write`], but read `clock` inside the write section,
        /// pass it to `update` and return it.
        ///
        /// The clock is read only once the odd sequence is visible. A reader
        /// whose own clock read is later than this one therefore sees the
        /// write and retries, so it never extrapolates a period in progress
        /// past the value this write stores.
        #[inline(always)]
        pub(crate) fn write_at(
            &self,
            clock: impl FnOnce() -> u64,
            update: impl FnOnce(&Self, u64),
        ) -> u64 {
            let sequence = self.sequence.load(Ordering::Relaxed);
            self.sequence
                .store(sequence.wrapping_add(1), Ordering::Relaxed);
            // Store-to-clock ordering needs a full fence.
            fence(Ordering::SeqCst);
            let at = clock();
            update(self, at);
            self.sequence
                .store(sequence.wrapping_add(2), Ordering::Release);
            at
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
            self.read_at(|| ()).0
        }

        /// Read all words consistently, and `clock` after loading them and
        /// before confirming that no write started meanwhile. Pairs with
        /// [`Self::write_at`].
        pub(crate) fn read_at<T>(&self, mut clock: impl FnMut() -> T) -> ([u32; N], T) {
            loop {
                let before = self.sequence.load(Ordering::Acquire);
                if before & 1 == 0 {
                    let values =
                        core::array::from_fn(|index| self.words[index].load(Ordering::Relaxed));
                    fence(Ordering::SeqCst);
                    let at = clock();
                    fence(Ordering::SeqCst);
                    if self.sequence.load(Ordering::Relaxed) == before {
                        return (values, at);
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
    /// registrations and written only by the executor's thread.
    pub(crate) struct ExecutorCounters {
        executor: usize,
        started: u64,
        words: Words<9>,
    }

    static EXECUTORS: Mutex<CriticalSectionRawMutex, RefCell<Vec<&'static ExecutorCounters>>> =
        Mutex::new(RefCell::new(Vec::new()));

    impl ExecutorCounters {
        pub(crate) const fn new(executor: usize, started: u64) -> Self {
            Self {
                executor,
                started,
                words: Words::new(),
            }
        }

        /// Counters of the executor `executor` (`Spawner::executor_id`),
        /// created on first use and kept for the program's lifetime.
        pub(crate) fn get(executor: usize) -> &'static Self {
            EXECUTORS.lock(|executors| {
                let mut executors = executors.borrow_mut();
                if let Some(counters) = executors
                    .iter()
                    .find(|counters| counters.executor == executor)
                {
                    return *counters;
                }
                let counters: &'static Self = Box::leak(Box::new(Self::new(executor, now())));
                executors.push(counters);
                counters
            })
        }

        pub(crate) fn executor(&self) -> usize {
            self.executor
        }

        /// Start a task poll; returns its start time in ticks.
        #[inline(always)]
        pub(crate) fn begin_poll(&self) -> u64 {
            self.begin(STATE_BUSY, now)
        }

        /// End the poll or wait in progress; returns its end time in ticks.
        #[inline(always)]
        pub(crate) fn end_period(&self) -> u64 {
            self.end(now)
        }

        #[inline(always)]
        fn begin(&self, state: u32, clock: impl FnOnce() -> u64) -> u64 {
            self.words.write_at(clock, |words, at| {
                words.set64(EXECUTOR_SINCE, at);
                let mut flags = words.get(EXECUTOR_STATE) & IDLE_MEASURED;
                if state == STATE_IDLE {
                    flags |= IDLE_MEASURED;
                }
                words.set(EXECUTOR_STATE, state | flags);
            })
        }

        #[inline(always)]
        fn end(&self, clock: impl FnOnce() -> u64) -> u64 {
            self.words.write_at(clock, |words, at| {
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
            })
        }

        /// Counters now, including the period in progress.
        pub(crate) fn sample(&self, spawner: usize) -> ExecutorMetrics {
            self.sample_at(spawner, now)
        }

        fn sample_at(&self, spawner: usize, clock: impl FnMut() -> u64) -> ExecutorMetrics {
            let (words, at) = self.words.read_at(clock);
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
                timestamp_nanoseconds: ticks_to_nanoseconds(at),
            }
        }
    }

    /// Idle-time hook of one executor, for the platform's executor loop.
    ///
    /// It is created from the executor's [`Spawner`] and, like it, is
    /// `!Send`, so it stays on the executor's thread, the only writer of the
    /// executor's counters.
    pub struct ExecutorActivity {
        counters: &'static ExecutorCounters,
        not_send: PhantomData<*mut ()>,
    }

    impl ExecutorActivity {
        /// Hook of the executor that owns `spawner`.
        pub fn new(spawner: Spawner) -> Self {
            Self {
                counters: ExecutorCounters::get(spawner.executor_id()),
                not_send: PhantomData,
            }
        }

        /// Run the executor's wait for work, counting its duration as idle.
        /// Call it between polls of the executor, never from a task.
        #[inline]
        pub fn measure_idle<R>(&self, wait: impl FnOnce() -> R) -> R {
            debug_assert_eq!(
                self.counters.words.get(EXECUTOR_STATE) & STATE_MASK,
                0,
                "measure_idle called during a poll"
            );
            self.counters.begin(STATE_IDLE, now);
            let result = wait();
            self.counters.end_period();
            result
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
            let counters = ExecutorCounters::new(1, 100);
            let at = |ticks| counters.sample_at(7, || ticks);
            assert_eq!(at(100).busy_nanoseconds, 0);
            assert_eq!(at(100).idle_nanoseconds, None);

            assert_eq!(counters.begin(STATE_BUSY, || 110), 110);
            // A poll in progress is already visible.
            assert_eq!(at(115).busy_nanoseconds, ticks_to_nanoseconds(5));
            assert_eq!(counters.end(|| 120), 120);
            counters.begin(STATE_IDLE, || 120);
            assert_eq!(at(150).idle_nanoseconds, Some(ticks_to_nanoseconds(30)));
            counters.end(|| 200);
            counters.begin(STATE_BUSY, || 205);
            counters.end(|| 206);

            let sample = at(1000);
            assert_eq!(sample.polls, 2);
            assert_eq!(sample.spawner, 7);
            assert_eq!(sample.executor, 1);
            assert_eq!(sample.busy_nanoseconds, ticks_to_nanoseconds(11));
            assert_eq!(sample.idle_nanoseconds, Some(ticks_to_nanoseconds(80)));
            assert_eq!(sample.started_nanoseconds, ticks_to_nanoseconds(100));
            assert_eq!(sample.timestamp_nanoseconds, ticks_to_nanoseconds(1000));
            // Idle stays reported once measured, and counters only grow.
            let later = at(2000);
            assert!(later.idle_nanoseconds.is_some());
            assert_eq!(
                ExecutorMetrics {
                    timestamp_nanoseconds: sample.timestamp_nanoseconds,
                    ..later
                },
                sample
            );
        }

        #[test]
        fn writers_read_the_clock_inside_the_write_section() {
            let counters = ExecutorCounters::new(1, 0);
            let assert_writing = || {
                assert_eq!(counters.words.sequence.load(Ordering::Relaxed) & 1, 1);
                10
            };
            counters.begin(STATE_IDLE, assert_writing);
            counters.end(assert_writing);
        }

        #[test]
        fn readers_retry_when_a_write_starts_before_their_clock_read() {
            // Interleave deterministically: the writer ends an idle period
            // between the reader's word loads and its clock read.
            let counters = ExecutorCounters::new(1, 0);
            counters.begin(STATE_IDLE, || 100);
            let mut reads = 0;
            let sample = counters.sample_at(0, || {
                reads += 1;
                if reads == 1 {
                    counters.end(|| 150);
                }
                200
            });
            assert_eq!(reads, 2, "the read must retry");
            assert_eq!(sample.idle_nanoseconds, Some(ticks_to_nanoseconds(50)));
            // Without the retry the reader would extrapolate the finished
            // idle period to 100 ticks, and later samples would decrease.
            assert_eq!(
                counters.sample_at(0, || 300).idle_nanoseconds,
                Some(ticks_to_nanoseconds(50))
            );
        }

        #[test]
        fn executor_counters_are_shared_per_executor() {
            let first = ExecutorCounters::get(usize::MAX - 1);
            let second = ExecutorCounters::get(usize::MAX - 1);
            let other = ExecutorCounters::get(usize::MAX - 2);
            assert!(core::ptr::eq(first, second));
            assert!(!core::ptr::eq(first, other));
            assert_eq!(first.executor(), usize::MAX - 1);
        }

        #[test]
        fn measured_idle_runs_the_wait() {
            let executor = alloc::boxed::Box::leak(alloc::boxed::Box::new(
                embassy_executor::raw::Executor::new(core::ptr::null_mut()),
            ));
            let spawner = executor.spawner();
            let activity = ExecutorActivity::new(spawner);
            assert_eq!(activity.measure_idle(|| 42), 42);
            let sample = ExecutorCounters::get(spawner.executor_id()).sample(0);
            assert!(sample.idle_nanoseconds.is_some());
            assert_eq!(sample.executor, spawner.executor_id());
        }

        #[test]
        fn tick_conversion_saturates() {
            assert_eq!(ticks_to_nanoseconds(0), 0);
            assert_eq!(ticks_to_nanoseconds(TICK_HERTZ), 1_000_000_000);
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

        /// Regression test: a reader sampling while the executor's thread
        /// alternates polls and waits must never see busy or idle time
        /// decrease. Before the clock was read inside the write section, this
        /// failed in about nine runs out of ten.
        #[test]
        fn executor_samples_never_decrease_while_written() {
            extern crate std;
            use portable_atomic::AtomicBool;
            const ITERATIONS: usize = 1_000_000;
            static COUNTERS: ExecutorCounters = ExecutorCounters::new(42, 0);
            static DONE: AtomicBool = AtomicBool::new(false);
            let writer = std::thread::spawn(|| {
                for _ in 0..ITERATIONS {
                    COUNTERS.begin_poll();
                    COUNTERS.end_period();
                    COUNTERS.begin(STATE_IDLE, now);
                    COUNTERS.end_period();
                }
                DONE.store(true, Ordering::Release);
            });
            let mut previous = COUNTERS.sample(0);
            let mut decreases = 0_u64;
            while !DONE.load(Ordering::Acquire) {
                let sample = COUNTERS.sample(0);
                if sample.busy_nanoseconds < previous.busy_nanoseconds
                    || sample.idle_nanoseconds.unwrap_or(0) < previous.idle_nanoseconds.unwrap_or(0)
                    || sample.timestamp_nanoseconds < previous.timestamp_nanoseconds
                {
                    decreases += 1;
                }
                let accounted = sample.busy_nanoseconds + sample.idle_nanoseconds.unwrap_or(0);
                assert!(accounted <= sample.timestamp_nanoseconds, "{sample:?}");
                previous = sample;
            }
            writer.join().unwrap();
            assert_eq!(decreases, 0, "samples decreased {decreases} times");
            assert_eq!(COUNTERS.sample(0).polls, ITERATIONS as u64);
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
        let executor = alloc::boxed::Box::leak(alloc::boxed::Box::new(
            embassy_executor::raw::Executor::new(core::ptr::null_mut()),
        ));
        assert_eq!(
            ExecutorActivity::new(executor.spawner()).measure_idle(|| 42),
            42
        );
    }
}
