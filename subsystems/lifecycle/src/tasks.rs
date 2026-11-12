//! Cooperative task stop with a monotonic deadline, using only the public task
//! manager API (relationships, signals and sleep).
//!
//! Tasks receive [`Signal::Termination`] and must finish by themselves. Embassy
//! cannot preempt or kill a task, so an unresponsive task is reported as
//! [`Error::Timeout`] and keeps every resource it references: nothing is
//! deallocated underneath it.

use crate::Error;
use alloc::vec::Vec;
use core::time::Duration;
use task::{Manager, Signal, TaskIdentifier};

/// Interval between liveness checks while waiting for tasks to finish.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Ask every descendant of `root` to terminate, except the subtrees rooted at
/// `keep` (runtime services stopped by the driver stage), and wait until they
/// are gone or `budget` elapsed on the monotonic clock `now`.
///
/// Tasks are tracked once discovered, so children that outlive their parent
/// (and therefore lose their place under `root`) are still waited for. Signals
/// go to the most recently discovered (deepest) tasks first and are resent on
/// every check; delivery is idempotent.
pub async fn stop(
    manager: &Manager,
    root: TaskIdentifier,
    keep: &[TaskIdentifier],
    now: impl Fn() -> Duration,
    budget: Duration,
) -> Result<(), Error> {
    let deadline = now().saturating_add(budget);
    let mut tracked: Vec<TaskIdentifier> = Vec::new();
    loop {
        discover(manager, root, keep, &mut tracked).await;

        let mut index = 0;
        while index < tracked.len() {
            if manager.get_parent(tracked[index]).await.is_ok() {
                index += 1;
            } else {
                tracked.swap_remove(index);
            }
        }
        if tracked.is_empty() {
            return Ok(());
        }

        for &task in tracked.iter().rev() {
            let _ = manager.send_signal(task, Signal::Termination).await;
        }

        if now() >= deadline {
            return Err(Error::Timeout);
        }
        Manager::sleep(POLL_INTERVAL).await;
    }
}

async fn discover(
    manager: &Manager,
    root: TaskIdentifier,
    keep: &[TaskIdentifier],
    tracked: &mut Vec<TaskIdentifier>,
) {
    let mut index = 0;
    let mut parent = root;
    loop {
        if let Ok(children) = manager.get_children(parent).await {
            for child in children {
                if child != root && !keep.contains(&child) && !tracked.contains(&child) {
                    tracked.push(child);
                }
            }
        }
        let Some(&next) = tracked.get(index) else {
            return;
        };
        parent = next;
        index += 1;
    }
}

/// Whether the current task was asked to terminate or interrupt.
///
/// Signals are not consumed. Tasks without a signal wakeup must poll this at a
/// bounded interval while blocked.
pub async fn termination_requested(manager: &Manager, task: TaskIdentifier) -> bool {
    matches!(
        manager.peek_signal(task).await,
        Ok(Some(Signal::Termination | Signal::Interrupt))
    )
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use core::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn clock() -> impl Fn() -> Duration {
        let start = Instant::now();
        move || start.elapsed()
    }

    #[task::test]
    async fn cooperative_tasks_stop_and_unresponsive_ones_time_out() {
        static RELEASED: AtomicBool = AtomicBool::new(false);
        static STOP_STUBBORN: AtomicBool = AtomicBool::new(false);
        let manager = task::test_manager();
        let root = manager.get_current_task_identifier().await;
        let spawner = manager.get_spawner(root).await.unwrap();

        let (service, service_identifier) = manager
            .spawn(root, "Service", Some(spawner), async |_| {
                while !STOP_STUBBORN.load(Ordering::Acquire) {
                    Manager::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();

        let (cooperative, _) = manager
            .spawn(root, "Guest", Some(spawner), async move |task| {
                let manager = task::test_manager();
                // A nested child is stopped too.
                let (child, _) = manager
                    .spawn(task, "Guest child", Some(spawner), async |task| {
                        let manager = task::test_manager();
                        while !termination_requested(manager, task).await {
                            Manager::sleep(Duration::from_millis(5)).await;
                        }
                    })
                    .await
                    .unwrap();
                while !termination_requested(manager, task).await {
                    Manager::sleep(Duration::from_millis(5)).await;
                }
                child.join().await;
                // Cleanup hook: handles are released before the task ends.
                RELEASED.store(true, Ordering::Release);
            })
            .await
            .unwrap();

        assert_eq!(
            stop(
                manager,
                root,
                &[service_identifier],
                clock(),
                Duration::from_secs(2)
            )
            .await,
            Ok(())
        );
        cooperative.join().await;
        assert!(RELEASED.load(Ordering::Acquire));
        // The kept service never received a signal and still runs.
        assert_eq!(manager.peek_signal(service_identifier).await, Ok(None));

        let (stubborn, stubborn_identifier) = manager
            .spawn(root, "Stubborn", Some(spawner), async |_| {
                while !STOP_STUBBORN.load(Ordering::Acquire) {
                    Manager::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        let started = Instant::now();
        assert_eq!(
            stop(
                manager,
                root,
                &[service_identifier],
                clock(),
                Duration::from_millis(100)
            )
            .await,
            Err(Error::Timeout)
        );
        assert!(started.elapsed() >= Duration::from_millis(100));
        // Timed-out tasks stay registered with their state intact.
        assert!(manager.get_parent(stubborn_identifier).await.is_ok());
        assert_eq!(
            manager.peek_signal(stubborn_identifier).await,
            Ok(Some(Signal::Termination))
        );

        STOP_STUBBORN.store(true, Ordering::Release);
        stubborn.join().await;
        service.join().await;
    }
}
