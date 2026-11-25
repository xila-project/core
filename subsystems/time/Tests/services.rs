//! Host-only integration tests for the `services` feature: the change
//! notification of the time manager, the privileged manual UTC set API and the
//! real-time clock write-back task.

#![cfg(all(feature = "services", target_os = "linux"))]
#![allow(
    clippy::disallowed_types,
    reason = "host-only test uses std atomics for the fake monotonic clock"
)]

use std::sync::atomic::{AtomicU64, Ordering};

use core::time::Duration;
use peripherals::real_time_clock::MockRealTimeClock;
use time::manual::{self, ManualError};
use time::{ClockSource, Manager, MonotonicClock, RealtimeSample, Timestamp};

struct Clock(AtomicU64);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::Relaxed))
    }
}

fn manager_at(millis: u64) -> (&'static Clock, Manager<'static>) {
    let clock: &'static Clock = Box::leak(Box::new(Clock(AtomicU64::new(millis))));
    let manager = Manager::with_clock(clock);
    (clock, manager)
}

fn sample(nanoseconds: i128, source: ClockSource) -> RealtimeSample {
    RealtimeSample {
        timestamp: Timestamp::from_nanos(nanoseconds),
        source,
        resolution: Duration::from_secs(1),
        accuracy: None,
    }
}

async fn current_task(task_manager: &task::Manager) -> task::TaskIdentifier {
    task_manager.get_current_task_identifier().await
}

#[test]
fn every_set_and_invalidate_bumps_the_generation() {
    let (_clock, manager) = manager_at(0);
    assert_eq!(manager.change_generation(), 0);
    let sample = time::RealtimeSample {
        timestamp: Timestamp::from_nanos(1_000_000_000),
        source: ClockSource::Manual,
        resolution: Duration::from_secs(1),
        accuracy: None,
    };
    manager.set_realtime(sample).unwrap();
    assert_eq!(manager.change_generation(), 1);
    manager.set_realtime(sample).unwrap();
    assert_eq!(manager.change_generation(), 2);
    // A rejected set leaves the generation untouched.
    let mut invalid = sample;
    invalid.resolution = Duration::ZERO;
    assert!(manager.set_realtime(invalid).is_err());
    assert_eq!(manager.change_generation(), 2);
    manager.invalidate();
    assert_eq!(manager.change_generation(), 3);
    assert_eq!(manager.synchronization(), None);
}

#[task::test]
async fn wait_for_change_returns_the_new_generation() {
    let (_clock, manager) = manager_at(0);
    let generation = manager.change_generation();
    let sample = time::RealtimeSample {
        timestamp: Timestamp::from_nanos(42_000_000_000),
        source: ClockSource::Manual,
        resolution: Duration::from_secs(1),
        accuracy: None,
    };
    manager.set_realtime(sample).unwrap();
    // The signal fired before the wait: it is not lost.
    assert_eq!(manager.wait_for_change(generation).await, generation + 1);
}

#[task::test]
async fn set_realtime_manual_requires_root() {
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let (_clock, manager) = manager_at(0);
    let task = current_task(task_manager).await;

    // A non-root user in its own group is refused.
    let group = users_manager.get_new_group_identifier().await.unwrap();
    users_manager.add_group(group, "guests", &[]).await.unwrap();
    let user = users_manager.get_new_user_identifier().await.unwrap();
    users_manager
        .add_user(user, "guest", group, "/users/guest")
        .await
        .unwrap();
    task_manager.set_user(task, user).await.unwrap();
    assert_eq!(
        manual::set_realtime_manual(
            task_manager,
            task,
            &manager,
            Timestamp::from_nanos(1_700_000_000_000_000_000)
        )
        .await,
        Err(ManualError::PermissionDenied)
    );
    assert_eq!(manager.synchronization(), None);

    // Root may set UTC manually; the source is recorded as manual and the
    // change is notified (the write-back trigger).
    let generation = manager.change_generation();
    task_manager
        .set_user(task, users::UserIdentifier::ROOT)
        .await
        .unwrap();
    manual::set_realtime_manual(
        task_manager,
        task,
        &manager,
        Timestamp::from_nanos(1_700_000_000_000_000_000),
    )
    .await
    .unwrap();
    let synchronization = manager.synchronization().unwrap();
    assert_eq!(synchronization.source, ClockSource::Manual);
    assert_eq!(synchronization.resolution, Duration::from_secs(1));
    assert_eq!(
        manager.realtime_now().unwrap().as_nanos(),
        1_700_000_000_000_000_000
    );
    assert_eq!(manager.wait_for_change(generation).await, generation + 1);

    // A subsecond input is reported with a nanosecond resolution.
    manual::set_realtime_manual(
        task_manager,
        task,
        &manager,
        Timestamp::from_nanos(1_700_000_000_123_456_789),
    )
    .await
    .unwrap();
    assert_eq!(
        manager.synchronization().unwrap().resolution,
        Duration::from_nanos(1)
    );
}

#[task::test]
async fn write_back_writes_on_change_and_periodically_only_from_writable_sources() {
    let task_manager = task::test_manager();
    let (_clock, manager) = manager_at(0);
    let manager: &'static Manager<'static> = Box::leak(Box::new(manager));
    let controller: &'static MockRealTimeClock = Box::leak(Box::new(MockRealTimeClock::new(None)));

    // Run the write-back task on the real test executor (a `Timer` future only
    // works there, not under `poll_once`), with a short rewrite interval.
    let task = current_task(task_manager).await;
    let spawner = task_manager.get_spawner(task).await.unwrap();
    task_manager
        .spawn(task, "write-back", Some(spawner), async move |_| {
            time::real_time_clock::write_back_to_real_time_clock_with_interval(
                manager,
                controller,
                Duration::from_millis(10),
            )
            .await
        })
        .await
        .unwrap();

    // Unsynchronized UTC is never written back.
    task::sleep(Duration::from_millis(50)).await;
    assert_eq!(controller.write_count(), 0);

    // A manual set is a UTC change: the write-back task wakes and stores it.
    manager
        .set_realtime(sample(1_700_000_000_000_000_000, ClockSource::Manual))
        .unwrap();
    task::sleep(Duration::from_millis(50)).await;
    assert!(
        controller.write_count() >= 1,
        "{}",
        controller.write_count()
    );
    assert_eq!(controller.time().unwrap().seconds, 1_700_000_000);

    // While the source stays manual, the interval timer rewrites periodically.
    let count = controller.write_count();
    task::sleep(Duration::from_millis(80)).await;
    assert!(
        controller.write_count() > count,
        "{count} -> {}",
        controller.write_count()
    );

    // A source of RTC is never written back: the clock keeps its own time.
    manager
        .set_realtime(sample(1_800_000_000_000_000_000, ClockSource::Rtc))
        .unwrap();
    task::sleep(Duration::from_millis(80)).await;
    let count = controller.write_count();
    task::sleep(Duration::from_millis(80)).await;
    assert_eq!(controller.write_count(), count);
    assert_eq!(controller.time().unwrap().seconds, 1_700_000_000);

    // The build-date fallback is never written back either.
    manager
        .set_realtime(sample(1_750_000_000_000_000_000, ClockSource::BuildDate))
        .unwrap();
    task::sleep(Duration::from_millis(80)).await;
    let count = controller.write_count();
    task::sleep(Duration::from_millis(80)).await;
    assert_eq!(controller.write_count(), count);
    assert_eq!(controller.time().unwrap().seconds, 1_700_000_000);

    // A later manual set is written again.
    manager
        .set_realtime(sample(1_900_000_000_000_000_000, ClockSource::Manual))
        .unwrap();
    task::sleep(Duration::from_millis(50)).await;
    assert!(controller.write_count() > count);
    assert_eq!(controller.time().unwrap().seconds, 1_900_000_000);
}
