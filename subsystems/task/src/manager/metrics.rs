// Metrics module - read-only activity snapshots for monitoring applications

use super::*;
use crate::metrics::MetricsSnapshot;

impl Manager {
    /// Sample the cumulative poll counters of every registered task and every
    /// executor with a spawner registration. Returns `None` without the
    /// `metrics` feature.
    ///
    /// Counters only grow (task counters restart with each registration, see
    /// [`crate::metrics::TaskMetrics::generation`]), so callers compute rates
    /// from the differences between two snapshots, with `saturating_sub`; see
    /// [`crate::metrics`] for how to compute executor and task loads. Each
    /// executor is read with its own timestamp, at the same instant as its
    /// counters, so `Δbusy + Δidle` stays within `Δtimestamp` up to rounding
    /// and cross-core extrapolation, described in [`crate::metrics`]: clamp
    /// loads to `[0, 1]`. The snapshot timestamp is read last, after every
    /// counter.
    ///
    /// Readers spin while an executor writes its counters, outside any
    /// critical section. Never call this from a task of an interrupt executor
    /// that can preempt an instrumented executor on the same core.
    pub async fn get_metrics(&self) -> Option<MetricsSnapshot> {
        #[cfg(feature = "metrics")]
        {
            use crate::metrics::{ExecutorCounters, TaskMetrics, ticks_to_nanoseconds};
            use alloc::vec::Vec;

            let inner = self.0.read().await;
            let tasks = inner
                .tasks
                .iter()
                .map(|(identifier, metadata)| {
                    let sample = metadata
                        .instrumentation
                        .map(|instrumentation| instrumentation.task.sample())
                        .unwrap_or_default();
                    TaskMetrics {
                        identifier: *identifier,
                        generation: metadata.generation,
                        name: metadata.name.clone(),
                        spawner: metadata.spawner_identifier,
                        executor: metadata
                            .instrumentation
                            .map(|instrumentation| instrumentation.executor.executor()),
                        polls: sample.polls,
                        busy_nanoseconds: ticks_to_nanoseconds(sample.busy),
                        longest_poll_nanoseconds: ticks_to_nanoseconds(sample.longest_poll),
                    }
                })
                .collect();
            drop(inner);
            // Copy the counters out of the critical section and sample them
            // after releasing it: readers spin while a writer is active, and
            // that writer may need the critical section on another core.
            let counters = self.1.lock(|spawners| {
                let spawners = spawners.borrow();
                let mut counters: Vec<(SpawnerIdentifier, &'static ExecutorCounters)> =
                    Vec::with_capacity(spawners.len());
                // Registrations iterate in increasing identifier order, so
                // each executor is reported under its lowest registration.
                for (identifier, state) in spawners.iter() {
                    if !counters
                        .iter()
                        .any(|(_, known)| core::ptr::eq(*known, state.counters))
                    {
                        counters.push((*identifier, state.counters));
                    }
                }
                counters
            });
            let executors = counters
                .into_iter()
                .map(|(identifier, counters)| counters.sample(identifier))
                .collect();
            Some(MetricsSnapshot {
                timestamp_nanoseconds: ticks_to_nanoseconds(crate::metrics::now()),
                tasks,
                executors,
            })
        }
        #[cfg(not(feature = "metrics"))]
        None
    }
}
