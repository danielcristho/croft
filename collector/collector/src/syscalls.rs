// Decodes x86_64 syscall numbers into names, and tracks their rate over time.

use std::collections::HashMap;

// unpack_key reverses collector-ebpf's pack_key(tgid, syscall_nr as u32).
pub fn unpack_key(key: u64) -> (u32, i64) {
    let tgid = (key >> 32) as u32;
    let syscall_nr = (key & 0xFFFF_FFFF) as u32 as i64;
    (tgid, syscall_nr)
}

// name decodes a syscall number; an unlisted one renders as syscall_<nr>.
pub fn name(nr: i64) -> String {
    match nr {
        0 => "read".into(),
        1 => "write".into(),
        2 => "open".into(),
        3 => "close".into(),
        9 => "mmap".into(),
        16 => "ioctl".into(),
        41 => "socket".into(),
        42 => "connect".into(),
        44 => "sendto".into(),
        45 => "recvfrom".into(),
        56 => "clone".into(),
        59 => "execve".into(),
        60 => "exit".into(),
        61 => "wait4".into(),
        202 => "futex".into(),
        231 => "exit_group".into(),
        257 => "openat".into(),
        other => format!("syscall_{other}"),
    }
}

// RateTracker turns a raw cumulative eBPF counter into a rate.
#[derive(Default)]
pub struct RateTracker {
    prev: HashMap<(u32, i64), (u64, u64)>,
}

impl RateTracker {
    pub fn rate_per_sec(&mut self, tgid: u32, syscall_nr: i64, count: u64, now_ns: u64) -> f64 {
        let key = (tgid, syscall_nr);
        let rate = match self.prev.get(&key) {
            Some(&(prev_count, prev_ns)) if now_ns > prev_ns => {
                let elapsed_secs = (now_ns - prev_ns) as f64 / 1_000_000_000.0;
                count.saturating_sub(prev_count) as f64 / elapsed_secs
            }
            _ => 0.0,
        };
        self.prev.insert(key, (count, now_ns));
        rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_key_reverses_pack_key() {
        let key = (7u64 << 32) | 16u64;
        assert_eq!(unpack_key(key), (7, 16));
    }

    #[test]
    fn first_observation_has_no_rate() {
        let mut rates = RateTracker::default();
        assert_eq!(rates.rate_per_sec(1, 16, 100, 1_000_000_000), 0.0);
    }

    #[test]
    fn rate_is_delta_over_elapsed_time() {
        let mut rates = RateTracker::default();
        rates.rate_per_sec(1, 16, 100, 1_000_000_000);
        let rate = rates.rate_per_sec(1, 16, 300, 3_000_000_000);
        assert_eq!(rate, 100.0); // 200 more calls over 2 seconds
    }

    #[test]
    fn reused_tgid_does_not_inherit_the_old_rate() {
        let mut rates = RateTracker::default();
        assert_eq!(rates.rate_per_sec(1, 16, 50_000, 1_000_000_000), 0.0);
    }
}
