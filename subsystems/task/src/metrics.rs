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
//! On Linux, macOS and Windows the metrics clock counts from its own first
//! read, not from `embassy_time::Instant`'s origin: compare metrics
//! timestamps only with each other.
//!
//! Without the feature, [`Manager::get_metrics`](crate::Manager::get_metrics)
//! returns `None` and [`ExecutorActivity::measure_idle`] only runs its
//! closure, so platforms and applications can use the API unconditionally at
//! no cost.
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
//! Every reported counter and executor timestamp never decreases. Consumers
//! sample [`Manager::get_metrics`](crate::Manager::get_metrics) periodically
//! and divide differences between two snapshots, computed with
//! `saturating_sub`:
//!
//! - Match executors between snapshots by [`ExecutorMetrics::executor`], not
//!   by [`ExecutorMetrics::spawner`]: an executor is reported under its lowest
//!   spawner registration, which can change.
//! - An executor's load over the window is `Δbusy / Δtimestamp` with that
//!   executor's own [`ExecutorMetrics::timestamp_nanoseconds`]. When
//!   [`ExecutorMetrics::idle_nanoseconds`] is available on both sides, prefer
//!   `1 − Δidle / Δtimestamp`: it also counts executor bookkeeping and
//!   foreign work on that core as load. Both include the poll or wait in
//!   progress, so they are continuous. They are close to, but not strictly
//!   within, `[0, 1]`: clamp them (see [Precision](#precision)).
//! - A task's share is `Δbusy / Δ`[`MetricsSnapshot::timestamp_nanoseconds`].
//!   Task busy time is added when a poll ends, so a long poll counts entirely
//!   in the window where it ends, and the shares of one executor's tasks can
//!   then sum above 1. Normalise per executor, grouping tasks by
//!   [`TaskMetrics::executor`]: when their sum exceeds 1, divide each of them
//!   by that sum rather than clamping each task separately. Compare
//!   [`TaskMetrics::generation`] to detect a reused task identifier.
//! - Tasks adopted rather than spawned by the manager have no counters, report
//!   zero polls, have `spawner == usize::MAX` and no executor.
//!
//! # Precision
//!
//! Counters are kept in clock ticks and each one is converted to nanoseconds
//! separately, rounding down. When `TICK_HERTZ` does not divide one billion,
//! as with a 32,768 Hz counter, the difference of two converted values can
//! exceed the converted difference by 1 ns, so `Δidle` can exceed
//! `Δtimestamp` by 1 ns over the same ticks. On multi-core targets, a reader
//! can also extrapolate a period in progress slightly past the value its
//! writer finally stores (see [Concurrency](#concurrency)); the excess is
//! kept rather than reported as a decrease. Use `saturating_sub` for every
//! difference and clamp ratios to `[0, 1]`.
//!
//! # Platform hook
//!
//! Executor busy time is derived from task polls and needs no platform code.
//! Idle time, the time an executor waits for work, can only be measured by the
//! executor loop. Platforms create an [`ExecutorActivity`] on the executor's
//! thread before its loop, and wrap their wait with
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
//! Once an executor's hook exists, its [`ExecutorMetrics::idle_nanoseconds`]
//! is `Some`, starting at 0; without the hook, it is `None`. The hook is
//! `!Send`, like the [`Spawner`](embassy_executor::Spawner) it is created
//! from:
//!
//! ```compile_fail,E0277
//! fn assert_send<T: Send>() {}
//! assert_send::<task::metrics::ExecutorActivity>();
//! ```
//!
//! # Concurrency
//!
//! Each executor's counters have a single writer, the executor's own thread:
//! task polls and [`ExecutorActivity::measure_idle`], whose handle cannot
//! leave its thread. Readers use a sequence lock built from 32-bit atomic
//! loads and stores, so it works without compare-and-swap or 64-bit atomics,
//! and they spin while a write is in progress, outside any critical section.
//! Never read the counters from code that preempts a writer on the same core:
//! an interrupt handler, or a task of an interrupt executor that preempts an
//! instrumented thread executor. Such a reader would spin forever.
//!
//! Writers read the clock inside their write section, which keeps a reader
//! from extrapolating a finished period on a single core and on x86. On
//! weakly ordered multi-core architectures, such as AArch64 and RISC-V,
//! fences do not order a system-register clock read after an earlier store,
//! so a reader on another core can still extrapolate a period in progress by
//! up to the writer's store latency. Reported values never decrease because
//! every reader clamps its sample to a per-executor high-water mark of busy
//! time, idle time and timestamp, updated with plain loads and stores inside a
//! short critical section.

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
    /// Executor running the task, as in [`ExecutorMetrics::executor`];
    /// `None` for adopted tasks.
    pub executor: Option<usize>,
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
    /// progress. Never decreases.
    pub busy_nanoseconds: u64,
    /// Time spent waiting for work, including a wait in progress, once the
    /// platform executor created its [`ExecutorActivity`]. Never decreases.
    pub idle_nanoseconds: Option<u64>,
    /// Clock value when the counters were created.
    pub started_nanoseconds: u64,
    /// Clock value at which these counters were read. Divide busy and idle
    /// differences by differences of this value. Never decreases.
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
    use core::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };
    use embassy_executor::Spawner;
    use portable_atomic::{AtomicU32, Ordering, fence};
    use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    mod clock {
        extern crate std;
        use std::{sync::OnceLock, time::Instant};

        /// The operating system's monotonic clock, in nanoseconds.
        pub const HERTZ: u64 = 1_000_000_000;

        /// Nanoseconds since the first read, an origin unrelated to
        /// `embassy_time::Instant`'s. Embassy's std clock takes its driver's
        /// global mutex on every read; this one is lock-free.
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

    /// Convert clock ticks to nanoseconds, rounding down and saturating.
    ///
    /// When [`TICK_HERTZ`] does not divide one billion, the difference of two
    /// converted values can exceed the converted difference by 1 ns.
    pub fn ticks_to_nanoseconds(ticks: u64) -> u64 {
        convert(ticks, TICK_HERTZ)
    }

    #[inline(always)]
    fn convert(ticks: u64, hertz: u64) -> u64 {
        if 1_000_000_000 % hertz == 0 {
            ticks.saturating_mul(1_000_000_000 / hertz)
        } else {
            let nanoseconds = u128::from(ticks) * 1_000_000_000 / u128::from(hertz);
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
        /// On a single core and on x86, the clock is read only once the odd
        /// sequence is visible, so a reader whose own clock read is later
        /// sees the write and retries instead of extrapolating a period in
        /// progress past the value this write stores. Weakly ordered
        /// architectures do not order a system-register clock read after the
        /// fence; readers clamp their samples for those.
        #[inline(always)]
        pub(crate) fn write_at(
            &self,
            clock: impl FnOnce() -> u64,
            update: impl FnOnce(&Self, u64),
        ) -> u64 {
            let sequence = self.sequence.load(Ordering::Relaxed);
            self.sequence
                .store(sequence.wrapping_add(1), Ordering::Relaxed);
            // Store-to-load ordering needs a full fence.
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
    const TASK_WORDS: usize = 6;

    /// Per-slot task counters, in ticks. Retained with their storage slot and
    /// reset when the slot is claimed by a new task.
    pub(crate) struct TaskCounters(Words<TASK_WORDS>);

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
                .write(|words| (0..TASK_WORDS).for_each(|index| words.set(index, 0)));
        }

        #[inline(always)]
        pub(crate) fn record_poll(&self, elapsed: u64) {
            self.0.write(|words| {
                words.add64(TASK_BUSY, elapsed);
                words.add64(TASK_POLLS, 1);
                if elapsed > words.get64(TASK_LONGEST) {
                    words.set64(TASK_LONGEST, elapsed);
                }
            });
        }

        pub(crate) fn sample(&self) -> TaskSample {
            let words = self.0.read();
            TaskSample {
                polls: get64(&words, TASK_POLLS),
                busy: get64(&words, TASK_BUSY),
                longest_poll: get64(&words, TASK_LONGEST),
            }
        }
    }

    /// Counters of one spawned task and of the executor polling it.
    #[derive(Clone, Copy)]
    pub(crate) struct TaskInstrumentation {
        pub(crate) task: &'static TaskCounters,
        pub(crate) executor: &'static ExecutorCounters,
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
    /// Set once the platform created the executor's idle hook.
    const IDLE_MEASURED: u32 = 0b100;

    /// Highest busy time, idle time and timestamp, in ticks, reported so far.
    #[derive(Clone, Copy, Default)]
    struct HighWater {
        busy: u64,
        idle: u64,
        at: u64,
    }

    /// Busy and idle counters of one executor, shared by its spawner
    /// registrations and written only by the executor's thread.
    pub(crate) struct ExecutorCounters {
        executor: usize,
        started: u64,
        words: Words<9>,
        /// Shared by readers and updated only inside its critical section,
        /// so no compare-and-swap is needed.
        high_water: Mutex<CriticalSectionRawMutex, Cell<HighWater>>,
    }

    static EXECUTORS: Mutex<CriticalSectionRawMutex, RefCell<Vec<&'static ExecutorCounters>>> =
        Mutex::new(RefCell::new(Vec::new()));

    impl ExecutorCounters {
        pub(crate) const fn new(executor: usize, started: u64) -> Self {
            Self {
                executor,
                started,
                words: Words::new(),
                high_water: Mutex::new(Cell::new(HighWater {
                    busy: 0,
                    idle: 0,
                    at: started,
                })),
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

        /// Report idle time from now on, starting at zero.
        fn measure_idle(&self) {
            self.words.write(|words| {
                words.set(EXECUTOR_STATE, words.get(EXECUTOR_STATE) | IDLE_MEASURED);
            });
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
            // Never report less than an earlier reader, which may have
            // extrapolated a period in progress past its final value.
            let reported = self.high_water.lock(|high_water| {
                let previous = high_water.get();
                let reported = HighWater {
                    busy: busy.max(previous.busy),
                    idle: idle.max(previous.idle),
                    at: at.max(previous.at),
                };
                high_water.set(reported);
                reported
            });
            ExecutorMetrics {
                spawner,
                executor: self.executor,
                polls: get64(&words, EXECUTOR_POLLS),
                busy_nanoseconds: ticks_to_nanoseconds(reported.busy),
                idle_nanoseconds: (words[EXECUTOR_STATE] & IDLE_MEASURED != 0)
                    .then(|| ticks_to_nanoseconds(reported.idle)),
                started_nanoseconds: ticks_to_nanoseconds(self.started),
                timestamp_nanoseconds: ticks_to_nanoseconds(reported.at),
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
        /// Hook of the executor that owns `spawner`. Create it on the
        /// executor's thread, outside a poll. From then on, the executor
        /// reports idle time, starting at zero.
        pub fn new(spawner: Spawner) -> Self {
            let counters = ExecutorCounters::get(spawner.executor_id());
            counters.measure_idle();
            Self {
                counters,
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
            // The longest poll is 64 bits wide: nanosecond clocks pass
            // 32 bits after about 4.3 seconds.
            assert_eq!(sample.longest_poll, u64::MAX / 4);
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
        fn creating_the_hook_reports_idle_from_zero() {
            let counters = ExecutorCounters::new(1, 0);
            assert_eq!(counters.sample_at(0, || 10).idle_nanoseconds, None);
            counters.measure_idle();
            assert_eq!(counters.sample_at(0, || 20).idle_nanoseconds, Some(0));
            counters.begin(STATE_BUSY, || 30);
            counters.end(|| 40);
            // Polls keep the flag.
            assert_eq!(counters.sample_at(0, || 50).idle_nanoseconds, Some(0));
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
            // idle period to 100 ticks.
            assert_eq!(
                counters.sample_at(0, || 300).idle_nanoseconds,
                Some(ticks_to_nanoseconds(50))
            );
        }

        /// On weakly ordered multi-core targets, a reader's clock read can
        /// land after the writer's final clock read although the reader did
        /// not see the write start. Simulate it with clocks out of order.
        #[test]
        fn readers_clamp_samples_extrapolated_past_the_final_value() {
            let counters = ExecutorCounters::new(1, 0);
            counters.measure_idle();
            counters.begin(STATE_BUSY, || 100);
            // The reader extrapolates the poll to 150 ticks.
            let early = counters.sample_at(0, || 150);
            assert_eq!(early.busy_nanoseconds, ticks_to_nanoseconds(50));
            // The writer's clock read ends the poll at 140.
            counters.end(|| 140);
            counters.begin(STATE_IDLE, || 140);
            // An unclamped sample would report 40 busy ticks and an earlier
            // timestamp.
            let late = counters.sample_at(0, || 145);
            assert_eq!(late.busy_nanoseconds, early.busy_nanoseconds);
            assert_eq!(late.timestamp_nanoseconds, early.timestamp_nanoseconds);
            assert_eq!(late.idle_nanoseconds, Some(ticks_to_nanoseconds(5)));
            // Once the real values pass the high-water mark they are
            // reported unchanged.
            counters.end(|| 160);
            counters.begin(STATE_BUSY, || 160);
            let after = counters.sample_at(0, || 175);
            assert_eq!(after.busy_nanoseconds, ticks_to_nanoseconds(55));
            assert_eq!(after.idle_nanoseconds, Some(ticks_to_nanoseconds(20)));
            assert_eq!(after.timestamp_nanoseconds, ticks_to_nanoseconds(175));
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
            let counters = ExecutorCounters::get(spawner.executor_id());
            assert!(counters.sample(0).idle_nanoseconds.is_some());
            assert_eq!(activity.measure_idle(|| 42), 42);
            let sample = counters.sample(0);
            assert!(sample.idle_nanoseconds.is_some());
            assert_eq!(sample.executor, spawner.executor_id());
        }

        #[test]
        fn tick_conversion_saturates() {
            assert_eq!(ticks_to_nanoseconds(0), 0);
            assert_eq!(ticks_to_nanoseconds(TICK_HERTZ), 1_000_000_000);
            assert_eq!(ticks_to_nanoseconds(u64::MAX), u64::MAX);
        }

        /// At 32,768 Hz a tick is 1953125 / 64 ns, so the rounding pattern
        /// repeats every 64 ticks: these ranges cover every case.
        #[test]
        fn differences_of_32768_hertz_conversions_exceed_by_at_most_one_nanosecond() {
            const HERTZ: u64 = 32_768;
            let mut exceeded = false;
            for offset in [0, 1 << 40] {
                for start in offset..offset + 128 {
                    assert!(convert(start + 1, HERTZ) > convert(start, HERTZ));
                    for ticks in 0..128 {
                        let difference = convert(start + ticks, HERTZ) - convert(start, HERTZ);
                        let exact = convert(ticks, HERTZ);
                        assert!(
                            difference == exact || difference == exact + 1,
                            "{start} + {ticks}"
                        );
                        exceeded |= difference > exact;
                    }
                }
            }
            assert!(exceeded);
            // The same tick count can differ by 1 ns: one tick of idle time
            // from 1 to 2 measures 30,518 ns, one tick of timestamp from 0 to
            // 1 measures 30,517 ns. Consumers subtract with saturation.
            assert_eq!(convert(2, HERTZ) - convert(1, HERTZ), 30_518);
            assert_eq!(convert(1, HERTZ) - convert(0, HERTZ), 30_517);
            assert_eq!(convert(HERTZ, HERTZ), 1_000_000_000);
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

        /// Time a reader may extrapolate past the writer's final value: none
        /// on x86, whose fences order clock reads; up to the store latency on
        /// weakly ordered hosts.
        const EXTRAPOLATION_TOLERANCE: u64 = if cfg!(target_arch = "x86_64") {
            0
        } else {
            10_000
        };

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
                assert!(
                    accounted <= sample.timestamp_nanoseconds + EXTRAPOLATION_TOLERANCE,
                    "{sample:?}"
                );
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
