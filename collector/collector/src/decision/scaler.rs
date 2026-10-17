//! Scaler turns syscall rates into replica counts, using a calibration and a target CPU limit.

use std::collections::HashSet;

use crate::pipeline::metrics::{SYSCALL_RATE, Sample};

// Calibration holds the parameters to turn a syscall rate into a replica count.
#[derive(Clone, Debug)]
pub struct Calibration {
    pub syscall: String,
    pub calls_per_request: f64,
    pub cpu_seconds_per_request: f64,
    pub target_utilization: f64,
}

// requests_per_sec sums the syscall rates of the given samples, filtering by the given containers and calibration.
pub fn requests_per_sec(
    samples: &[Sample],
    containers: &HashSet<String>,
    calibration: &Calibration,
) -> f64 {
    samples
        .iter()
        .filter(|s| s.name == SYSCALL_RATE)
        .filter(|s| s.labels.get("syscall") == Some(&calibration.syscall))
        .filter(|s| {
            s.labels
                .get("container_id")
                .is_some_and(|id| containers.contains(id))
        })
        .fold(0.0, |total, s| total + s.value)
        / calibration.calls_per_request
}

// needed returns the number of replicas needed to handle the given requests per second, given the CPU limit and calibration, clamped to [1, max_replicas].
pub fn needed(
    requests_per_sec: f64,
    cpu_limit_cores: f64,
    calibration: &Calibration,
    max_replicas: u32,
) -> u32 {
    let demand = requests_per_sec * calibration.cpu_seconds_per_request;
    let replicas = (demand / (cpu_limit_cores * calibration.target_utilization)).ceil();
    (replicas as u32).clamp(1, max_replicas)
}

// Confirm waits for `polls` consecutive polls asking for more replicas than run, so one noisy poll scales nothing.
pub struct Confirm {
    polls: u32,
    streak: u32,
}

impl Confirm {
    pub fn new(polls: u32) -> Self {
        Self { polls, streak: 0 }
    }

    pub fn step(&mut self, needed: u32, current: u32) -> Option<u32> {
        if needed <= current {
            self.streak = 0;
            return None;
        }
        self.streak += 1;
        if self.streak < self.polls {
            return None;
        }
        self.streak = 0;
        Some(needed)
    }
}

// cpu_cores reads a Kubernetes CPU quantity such as "1", "1.5" or "500m".
pub fn cpu_cores(quantity: &str) -> Option<f64> {
    match quantity.strip_suffix('m') {
        Some(millis) => millis.parse::<f64>().ok().map(|m| m / 1000.0),
        None => quantity.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn calibration() -> Calibration {
        Calibration {
            syscall: "sendto".into(),
            calls_per_request: 2.0,
            cpu_seconds_per_request: 0.05,
            target_utilization: 0.7,
        }
    }

    fn sample(container: &str, syscall: &str, value: f64) -> Sample {
        Sample {
            name: SYSCALL_RATE.into(),
            labels: BTreeMap::from([
                ("container_id".to_string(), container.to_string()),
                ("syscall".to_string(), syscall.to_string()),
            ]),
            value,
        }
    }

    #[test]
    fn rate_counts_only_the_deployment_and_the_syscall() {
        let samples = [
            sample("a", "sendto", 40.0),
            sample("b", "sendto", 20.0),
            sample("a", "read", 99.0),
            sample("x", "sendto", 500.0),
        ];
        let ours = HashSet::from(["a".to_string(), "b".to_string()]);
        assert_eq!(requests_per_sec(&samples, &ours, &calibration()), 30.0);
    }

    #[test]
    fn needed_follows_offered_cpu() {
        assert_eq!(needed(75.0, 1.0, &calibration(), 10), 6);
        assert_eq!(needed(0.0, 1.0, &calibration(), 10), 1);
        assert_eq!(needed(1000.0, 1.0, &calibration(), 10), 10);
    }

    #[test]
    fn confirm_needs_consecutive_polls() {
        let mut confirm = Confirm::new(2);
        assert_eq!(confirm.step(6, 1), None);
        assert_eq!(confirm.step(1, 1), None);
        assert_eq!(confirm.step(6, 1), None);
        assert_eq!(confirm.step(7, 1), Some(7));
        assert_eq!(confirm.step(7, 7), None);
    }

    #[test]
    fn reads_cpu_quantities() {
        assert_eq!(cpu_cores("1"), Some(1.0));
        assert_eq!(cpu_cores("1.5"), Some(1.5));
        assert_eq!(cpu_cores("500m"), Some(0.5));
        assert_eq!(cpu_cores("lots"), None);
    }
}
