//! Per-second rates from cumulative counters, keyed by what each counter measures.

use std::{collections::HashMap, hash::Hash};

use crate::sensing::clock::NANOS_PER_SEC;

struct Seen {
    value: u64,
    at_ns: u64,
    poll: u64,
}

pub struct DeltaTracker<K> {
    prev: HashMap<K, Seen>,
    poll: u64,
}

// DeltaTracker tracks the previous values of counters and the time they were seen, so that it can compute per-second rates from cumulative counters.
impl<K> Default for DeltaTracker<K> {
    fn default() -> Self {
        Self {
            prev: HashMap::new(),
            poll: 0,
        }
    }
}

// DeltaTracker<K> is generic over K, but K must implement Hash and Eq for use in the HashMap.
impl<K: Hash + Eq> DeltaTracker<K> {
    pub fn per_sec(&mut self, key: K, value: u64, now_ns: u64) -> Option<f64> {
        let rate = self
            .prev
            .get(&key)
            .filter(|prev| now_ns > prev.at_ns)
            .map(|prev| {
                let elapsed_secs = (now_ns - prev.at_ns) as f64 / NANOS_PER_SEC as f64;
                value.saturating_sub(prev.value) as f64 / elapsed_secs
            });
        let poll = self.poll;
        self.prev.insert(
            key,
            Seen {
                value,
                at_ns: now_ns,
                poll,
            },
        );
        rate
    }

    // end_poll forgets keys not seen this poll, such as the counters of exited processes.
    pub fn end_poll(&mut self) {
        let poll = self.poll;
        self.prev.retain(|_, seen| seen.poll == poll);
        self.poll += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_has_no_rate() {
        let mut deltas = DeltaTracker::default();
        assert_eq!(deltas.per_sec("a", 100, NANOS_PER_SEC), None);
    }

    #[test]
    fn rate_is_delta_over_elapsed_time() {
        let mut deltas = DeltaTracker::default();
        deltas.per_sec("a", 100, NANOS_PER_SEC);
        assert_eq!(deltas.per_sec("a", 300, 3 * NANOS_PER_SEC), Some(100.0));
    }

    #[test]
    fn keys_missing_from_a_poll_are_forgotten() {
        let mut deltas = DeltaTracker::default();
        deltas.per_sec("gone", 100, NANOS_PER_SEC);
        deltas.end_poll();
        deltas.end_poll();
        assert_eq!(deltas.per_sec("gone", 300, 3 * NANOS_PER_SEC), None);
    }

    #[test]
    fn keys_seen_every_poll_are_kept() {
        let mut deltas = DeltaTracker::default();
        deltas.per_sec("alive", 100, NANOS_PER_SEC);
        deltas.end_poll();
        assert_eq!(deltas.per_sec("alive", 300, 3 * NANOS_PER_SEC), Some(100.0));
    }
}
