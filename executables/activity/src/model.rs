// Window notifications can coalesce; queued actions must not wait for a slow
// metrics refresh if a notification races waker registration.
pub fn bounded_wait(
    deadline: core::time::Duration,
    now: core::time::Duration,
) -> core::time::Duration {
    deadline
        .saturating_sub(now)
        .min(core::time::Duration::from_millis(100))
}

use alloc::string::String;

#[derive(Debug)]
pub struct Config {
    pub interval_ms: u64,
}
impl Config {
    pub fn parse(args: &[String]) -> Result<Self, ()> {
        let interval_ms = match args {
            [] => 1000,
            [flag, value] if flag == "--interval-ms" => value.parse().map_err(|_| ())?,
            _ => return Err(()),
        };
        if !(100..=10000).contains(&interval_ms) {
            return Err(());
        }
        Ok(Self { interval_ms })
    }
}

pub struct MemorySample {
    pub used: usize,
    pub capacity: Option<usize>,
}
impl MemorySample {
    pub fn new(used: usize, capacity: usize) -> Self {
        Self {
            used,
            capacity: (capacity != 0 && used <= capacity).then_some(capacity),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::ToString, vec};
    #[test]
    fn refresh_configuration_is_bounded() {
        assert_eq!(Config::parse(&[]).unwrap().interval_ms, 1000);
        for valid in [100, 250, 10000] {
            let args = vec!["--interval-ms".to_string(), valid.to_string()];
            assert_eq!(Config::parse(&args).unwrap().interval_ms, valid);
        }
        for args in [
            vec!["--interval-ms"],
            vec!["--bad"],
            vec!["--interval-ms", "0"],
            vec!["--interval-ms", "99"],
            vec!["--interval-ms", "10001"],
            vec!["--interval-ms", "x"],
            vec!["--interval-ms", "100", "extra"],
        ] {
            assert!(
                Config::parse(
                    &args
                        .into_iter()
                        .map(ToString::to_string)
                        .collect::<alloc::vec::Vec<_>>()
                )
                .is_err()
            );
        }
    }
    #[test]
    fn slow_sampling_still_services_queued_input_promptly() {
        use core::time::Duration;
        assert_eq!(
            bounded_wait(Duration::from_secs(10), Duration::ZERO),
            Duration::from_millis(100)
        );
        assert_eq!(
            bounded_wait(Duration::from_millis(25), Duration::ZERO),
            Duration::from_millis(25)
        );
        assert_eq!(
            bounded_wait(Duration::ZERO, Duration::from_secs(1)),
            Duration::ZERO
        );
    }

    #[test]
    fn unavailable_capacity_is_explicit() {
        assert_eq!(MemorySample::new(50, 0).capacity, None);
        assert_eq!(MemorySample::new(50, 100).capacity, Some(100));
        assert_eq!(MemorySample::new(101, 100).capacity, None);
    }
}
