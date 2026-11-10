//! Scaler turns syscall rates into replica counts, using a calibration and a target CPU limit.

use std::collections::{HashSet, VecDeque};

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

// Repeats holds back a decision equal to the last one, which a dry run makes on every confirm window.
#[derive(Default)]
pub struct Repeats {
    last: Option<(u32, u32)>,
}

impl Repeats {
    pub fn is_new(&mut self, from: u32, to: u32) -> bool {
        let new = self.last != Some((from, to));
        self.last = Some((from, to));
        new
    }

    // settle forgets the last decision once the need matches the replicas, so the next one shows again.
    pub fn settle(&mut self, needed: u32, current: u32) {
        if needed == current {
            self.last = None;
        }
    }
}

// ScaleDown follows the HPA's stabilization: it lowers replicas to the highest need seen over a full window.
pub struct ScaleDown {
    polls: usize,
    needs: VecDeque<u32>,
}

impl ScaleDown {
    pub fn new(polls: usize) -> Self {
        Self {
            polls: polls.max(1),
            needs: VecDeque::new(),
        }
    }

    pub fn step(&mut self, needed: u32, current: u32) -> Option<u32> {
        if self.needs.len() == self.polls {
            self.needs.pop_front();
        }
        self.needs.push_back(needed);
        if self.needs.len() < self.polls {
            return None;
        }
        let highest = self.needs.iter().copied().max()?;
        (highest < current).then_some(highest)
    }

    // restart empties the window, since a poll that missed a collector undercounts the need.
    pub fn restart(&mut self) {
        self.needs.clear();
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

    #[test]
    fn repeats_shows_a_decision_once_until_the_need_settles() {
        let mut repeats = Repeats::default();
        assert!(repeats.is_new(1, 2));
        assert!(!repeats.is_new(1, 2));
        assert!(repeats.is_new(1, 3));
        repeats.settle(2, 1);
        assert!(!repeats.is_new(1, 3));
        repeats.settle(1, 1);
        assert!(repeats.is_new(1, 3));
    }

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
    fn scale_down_waits_for_a_full_window() {
        let mut down = ScaleDown::new(3);
        assert_eq!(down.step(1, 5), None);
        assert_eq!(down.step(1, 5), None);
        assert_eq!(down.step(1, 5), Some(1));
    }

    #[test]
    fn scale_down_keeps_the_highest_need_of_the_window() {
        let mut down = ScaleDown::new(3);
        assert_eq!(down.step(1, 5), None);
        assert_eq!(down.step(4, 5), None);
        assert_eq!(down.step(1, 5), Some(4));
        assert_eq!(down.step(1, 4), None);
        assert_eq!(down.step(1, 4), Some(1));
    }

    #[test]
    fn scale_down_holds_replicas_the_need_reaches() {
        let mut down = ScaleDown::new(2);
        assert_eq!(down.step(5, 5), None);
        assert_eq!(down.step(5, 5), None);
        assert_eq!(down.step(7, 5), None);
    }

    #[test]
    fn scale_down_restart_empties_the_window() {
        let mut down = ScaleDown::new(2);
        assert_eq!(down.step(1, 5), None);
        down.restart();
        assert_eq!(down.step(1, 5), None);
        assert_eq!(down.step(1, 5), Some(1));
    }

    #[test]
    fn reads_cpu_quantities() {
        assert_eq!(cpu_cores("1"), Some(1.0));
        assert_eq!(cpu_cores("1.5"), Some(1.5));
        assert_eq!(cpu_cores("500m"), Some(0.5));
        assert_eq!(cpu_cores("lots"), None);
    }
}
