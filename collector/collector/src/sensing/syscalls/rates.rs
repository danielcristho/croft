//! Turns cumulative eBPF syscall counters into per-second rates.

use serde::{Deserialize, Serialize};

use super::{SyscallCount, names};
use crate::pipeline::delta::DeltaTracker;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyscallRate {
    pub name: String,
    pub rate_per_sec: f64,
}

#[derive(Default)]
pub struct RateTracker {
    deltas: DeltaTracker<(u32, u32)>,
}

// RateTracker keeps track of the previous counts for each process and syscall, so it can compute rates per second.
impl RateTracker {
    // rates_for returns one process's syscall rates, busiest first.
    pub fn rates_for(
        &mut self,
        tgid: u32,
        counts: &[SyscallCount],
        now_ns: u64,
    ) -> Vec<SyscallRate> {
        let mut rates: Vec<SyscallRate> = counts
            .iter()
            .map(|c| SyscallRate {
                name: names::name(c.nr),
                rate_per_sec: self.rate_per_sec(tgid, c, now_ns),
            })
            .collect();
        rates.sort_by(|a, b| b.rate_per_sec.total_cmp(&a.rate_per_sec));
        rates
    }

    pub fn end_poll(&mut self) {
        self.deltas.end_poll();
    }

    // rate_per_sec returns 0 on the first observation, having no previous count to diff.
    fn rate_per_sec(&mut self, tgid: u32, sample: &SyscallCount, now_ns: u64) -> f64 {
        self.deltas
            .per_sec((tgid, sample.nr), sample.count, now_ns)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensing::clock::NANOS_PER_SEC;

    const IOCTL: u32 = 16;

    fn count(count: u64) -> SyscallCount {
        SyscallCount { nr: IOCTL, count }
    }

    #[test]
    fn first_observation_has_no_rate() {
        let mut rates = RateTracker::default();
        assert_eq!(rates.rate_per_sec(1, &count(100), NANOS_PER_SEC), 0.0);
    }

    #[test]
    fn rates_are_sorted_busiest_first() {
        let mut rates = RateTracker::default();
        let first = [SyscallCount { nr: 0, count: 10 }, count(10)];
        let second = [SyscallCount { nr: 0, count: 20 }, count(110)];
        rates.rates_for(1, &first, NANOS_PER_SEC);
        let rated = rates.rates_for(1, &second, 2 * NANOS_PER_SEC);
        assert_eq!(rated[0].name, "ioctl");
        assert_eq!(rated[1].name, "read");
    }
}
