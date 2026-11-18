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
    /// from the differences between two snapshots; see [`crate::metrics`] for
    /// how to compute executor and task loads. Each executor is read with its
    /// own timestamp, at the same instant as its counters, so
    /// `Δbusy + Δidle ≤ Δtimestamp` holds exactly per executor. The snapshot
    /// timestamp is read last, after every counter.
    ///
    /// Executor counters are read inside the manager's critical section.
    /// Never call this from a task of an interrupt executor that can preempt
    /// an instrumented executor on the same core.
    pub async fn get_metrics(&self) -> Option<MetricsSnapshot> {
        #[cfg(feature = "metrics")]
        {
            use crate::metrics::{ExecutorMetrics, TaskMetrics, ticks_to_nanoseconds};
            use alloc::vec::Vec;

            let inner = self.0.read().await;
            let tasks = inner
                .tasks
                .iter()
                .map(|(identifier, metadata)| {
                    let sample = metadata
                        .counters
                        .map(|counters| counters.sample())
                        .unwrap_or_default();
                    TaskMetrics {
                        identifier: *identifier,
                        generation: metadata.generation,
                        name: metadata.name.clone(),
                        spawner: metadata.spawner_identifier,
                        polls: sample.polls,
                        busy_nanoseconds: ticks_to_nanoseconds(sample.busy),
                        longest_poll_nanoseconds: ticks_to_nanoseconds(sample.longest_poll),
                    }
                })
                .collect();
            drop(inner);
            let executors = self.1.lock(|spawners| {
                let spawners = spawners.borrow();
                let mut executors: Vec<ExecutorMetrics> = Vec::with_capacity(spawners.len());
                // Registrations iterate in increasing identifier order, so
                // each executor is reported under its lowest registration.
                for (identifier, state) in spawners.iter() {
                    let executor = state.activity.executor();
                    if !executors.iter().any(|metrics| metrics.executor == executor) {
                        executors.push(state.activity.sample(*identifier));
                    }
                }
                executors
            });
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
