//! Per-container signals of the latest poll in Prometheus text format, written by the collector and read by the scaler.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, RwLock},
};

use crate::{
    inspect::{
        process::ProcessInfo,
        resources::{CgroupUsage, ProcessUsage},
    },
    sensing::{
        operands::{Destination, ProcessOperands, socket_direction},
        syscalls::{self, SyscallRate},
    },
};

pub const CPU_CORES: &str = "croft_container_cpu_cores";
pub const CPU_PRESSURE: &str = "croft_container_cpu_pressure_avg10";
pub const CPU_THROTTLED: &str = "croft_container_cpu_throttled_fraction";
pub const RUNQUEUE: &str = "croft_container_runqueue_wait";
pub const INFLIGHT: &str = "croft_container_inflight_requests";
pub const SYSCALL_RATE: &str = "croft_container_syscall_rate";
pub const SYSCALL_BYTES: &str = "croft_container_syscall_bytes_total";
pub const CONNECTS: &str = "croft_container_connects_total";
pub const SOCKET_IO: &str = "croft_container_socket_io_total";
pub const POLL_UNIX_MS: &str = "croft_poll_unix_ms";
pub const ACCEPT_QUEUE: &str = "croft_container_accept_queue";

#[derive(Debug, Default, PartialEq)]
pub struct ContainerSignals {
    pub cpu_cores: Option<f64>,
    pub cpu_pressure: Option<f64>,
    pub cpu_throttled: Option<f64>,
    pub runqueue: Option<f64>,
    pub inflight: Option<f64>,
    pub syscalls: BTreeMap<String, f64>,
    pub syscall_bytes: BTreeMap<String, u64>,
    pub connects: BTreeMap<Destination, u64>,
    pub socket_io: BTreeMap<&'static str, u64>,
    pub listeners: BTreeMap<u64, u64>,
}

// Field reads one gauge of a container.
type Field = fn(&ContainerSignals) -> Option<f64>;

// Snapshot sums a container's processes; cgroup values are the container's own and are set, not summed.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub containers: BTreeMap<String, ContainerSignals>,
}

impl Snapshot {
    pub fn add(
        &mut self,
        process: &ProcessInfo,
        usage: &ProcessUsage,
        cgroup: Option<&CgroupUsage>,
        rates: &[SyscallRate],
        operands: ProcessOperands,
    ) {
        let Some(id) = &process.container_id else {
            return;
        };
        let signals = self.containers.entry(id.clone()).or_default();
        if let Some(cgroup) = cgroup {
            signals.cpu_cores = cgroup.cpu_cores;
            signals.cpu_pressure = cgroup.cpu_pressure_avg10;
            signals.cpu_throttled = cgroup.cpu_throttled_fraction;
        }
        if let Some(runqueue) = usage.runqueue_s {
            *signals.runqueue.get_or_insert(0.0) += runqueue;
        }
        if let Some(inflight) = operands.inflight {
            *signals.inflight.get_or_insert(0.0) += inflight as f64;
        }
        signals.listeners.extend(operands.listeners.iter().copied());
        for rate in rates {
            *signals.syscalls.entry(rate.name.clone()).or_default() += rate.rate_per_sec;
        }
        for bytes in operands.bytes {
            *signals
                .syscall_bytes
                .entry(syscalls::name(bytes.nr))
                .or_default() += bytes.count;
        }
        for &(destination, count) in operands.connects {
            *signals.connects.entry(destination).or_default() += count;
        }
        for io in operands.socket_io {
            *signals
                .socket_io
                .entry(socket_direction(io.nr))
                .or_default() += io.count;
        }
    }

    pub fn render(&self, poll_unix_ms: u128) -> String {
        let mut text = format!("# TYPE {POLL_UNIX_MS} gauge\n{POLL_UNIX_MS} {poll_unix_ms}\n");
        let gauges: [(&str, Field); 6] = [
            (CPU_CORES, |s| s.cpu_cores),
            (CPU_PRESSURE, |s| s.cpu_pressure),
            (CPU_THROTTLED, |s| s.cpu_throttled),
            (RUNQUEUE, |s| s.runqueue),
            (INFLIGHT, |s| s.inflight),
            (ACCEPT_QUEUE, |s| {
                (!s.listeners.is_empty()).then(|| s.listeners.values().sum::<u64>() as f64) // +
            }),
        ];
        for (name, value) in gauges {
            let _ = writeln!(text, "# TYPE {name} gauge");
            for (id, signals) in &self.containers {
                if let Some(value) = value(signals) {
                    let _ = writeln!(text, "{name}{{container_id=\"{id}\"}} {value}");
                }
            }
        }
        let _ = writeln!(text, "# TYPE {SYSCALL_RATE} gauge");
        for (id, signals) in &self.containers {
            for (syscall, rate) in &signals.syscalls {
                let _ = writeln!(
                    text,
                    "{SYSCALL_RATE}{{container_id=\"{id}\",syscall=\"{syscall}\"}} {rate}"
                );
            }
        }
        let _ = writeln!(text, "# TYPE {SYSCALL_BYTES} counter");
        for (id, signals) in &self.containers {
            for (syscall, bytes) in &signals.syscall_bytes {
                let _ = writeln!(
                    text,
                    "{SYSCALL_BYTES}{{container_id=\"{id}\",syscall=\"{syscall}\"}} {bytes}"
                );
            }
        }
        let _ = writeln!(text, "# TYPE {CONNECTS} counter");
        for (id, signals) in &self.containers {
            for ((address, port), count) in &signals.connects {
                let _ = writeln!(
                    text,
                    "{CONNECTS}{{container_id=\"{id}\",address=\"{address}\",port=\"{port}\"}} {count}"
                );
            }
        }
        let _ = writeln!(text, "# TYPE {SOCKET_IO} counter");
        for (id, signals) in &self.containers {
            for (direction, count) in &signals.socket_io {
                let _ = writeln!(
                    text,
                    "{SOCKET_IO}{{container_id=\"{id}\",direction=\"{direction}\"}} {count}"
                );
            }
        }
        text
    }
}

// Published holds the latest rendered snapshot for the HTTP server.
pub type Published = Arc<RwLock<String>>;

#[derive(Debug, PartialEq)]
pub struct Sample {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub value: f64,
}

// parse reads the lines render writes; comments and lines it cannot read are skipped.
pub fn parse(text: &str) -> Vec<Sample> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<Sample> {
    let (series, value) = line.rsplit_once(' ')?;
    let value = value.parse().ok()?;
    let (name, labels) = match series.split_once('{') {
        Some((name, rest)) => (name, rest.strip_suffix('}')?),
        None => (series, ""),
    };
    let labels = labels
        .split(',')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, quoted) = pair.split_once('=')?;
            Some((key.to_string(), quoted.trim_matches('"').to_string()))
        })
        .collect::<Option<_>>()?;
    Some(Sample {
        name: name.to_string(),
        labels,
        value,
    })
}

#[cfg(test)]
mod tests {
    use collector_common::SOCKET_READ;

    use super::*;
    use crate::sensing::syscalls::SyscallCount;

    fn process(id: &str) -> ProcessInfo {
        ProcessInfo {
            container_id: Some(id.into()),
            ..ProcessInfo::default()
        }
    }

    fn rate(name: &str, rate_per_sec: f64) -> SyscallRate {
        SyscallRate {
            name: name.into(),
            rate_per_sec,
        }
    }

    #[test]
    fn processes_of_one_container_add_up() {
        let mut snapshot = Snapshot::default();
        let cgroup = CgroupUsage {
            cpu_cores: Some(0.9),
            ..CgroupUsage::default()
        };
        let waiting = |runqueue_s| ProcessUsage {
            runqueue_s: Some(runqueue_s),
            ..ProcessUsage::default()
        };
        snapshot.add(
            &process("a"),
            &waiting(0.5),
            Some(&cgroup),
            &[rate("sendto", 10.0)],
            ProcessOperands::default(),
        );
        snapshot.add(
            &process("a"),
            &waiting(1.25),
            Some(&cgroup),
            &[rate("sendto", 5.0), rate("read", 1.0)],
            ProcessOperands::default(),
        );
        snapshot.add(
            &ProcessInfo::default(),
            &waiting(9.0),
            None,
            &[rate("sendto", 99.0)],
            ProcessOperands::default(),
        );
        let a = &snapshot.containers["a"];
        assert_eq!(a.syscalls["sendto"], 15.0);
        assert_eq!(a.runqueue, Some(1.75));
        assert_eq!(a.cpu_cores, Some(0.9));
        assert_eq!(snapshot.containers.len(), 1);
    }

    #[test]
    fn render_and_parse_round_trip() {
        let mut snapshot = Snapshot::default();
        let cgroup = CgroupUsage {
            cpu_cores: Some(0.5),
            cpu_pressure_avg10: Some(12.5),
            ..CgroupUsage::default()
        };
        snapshot.add(
            &process("abc"),
            &ProcessUsage::default(),
            Some(&cgroup),
            &[rate("sendto", 34.0)],
            ProcessOperands::default(),
        );
        let samples = parse(&snapshot.render(1_700_000_000_000));
        let find = |name: &str| samples.iter().find(|s| s.name == name).map(|s| s.value);
        assert_eq!(find(POLL_UNIX_MS), Some(1.7e12));
        assert_eq!(find(CPU_CORES), Some(0.5));
        assert_eq!(find(CPU_PRESSURE), Some(12.5));
        assert_eq!(find(CPU_THROTTLED), None);
        assert_eq!(find(RUNQUEUE), None);
        let sendto = samples.iter().find(|s| s.name == SYSCALL_RATE).unwrap();
        assert_eq!(sendto.labels["container_id"], "abc");
        assert_eq!(sendto.labels["syscall"], "sendto");
        assert_eq!(sendto.value, 34.0);
    }

    #[test]
    fn operands_add_up_per_container_and_round_trip() {
        let dns = ("10.43.0.10".parse().unwrap(), 53);
        let backend = ("fd00::1".parse().unwrap(), 8080);
        let bytes = [SyscallCount { nr: 1, count: 100 }];
        let reads = [SyscallCount {
            nr: SOCKET_READ,
            count: 3,
        }];
        let first = [(dns, 2)];
        let second = [(dns, 5), (backend, 1)];
        let mut snapshot = Snapshot::default();
        for connects in [&first[..], &second[..]] {
            snapshot.add(
                &process("a"),
                &ProcessUsage::default(),
                None,
                &[],
                ProcessOperands {
                    bytes: &bytes,
                    connects,
                    socket_io: &reads,
                    inflight: Some(3), // two processes of container a, three each, sum to 6
                    listeners: &[(42, 4), (43, 1)],
                },
            );
        }
        let samples = parse(&snapshot.render(0));
        let find = |name: &str, label: &str, value: &str| {
            samples
                .iter()
                .find(|s| s.name == name && s.labels.get(label).is_some_and(|v| v == value))
                .map(|s| s.value)
        };
        assert_eq!(find(SYSCALL_BYTES, "syscall", "write"), Some(200.0));
        assert_eq!(find(SOCKET_IO, "direction", "read"), Some(6.0));
        assert_eq!(find(CONNECTS, "address", "10.43.0.10"), Some(7.0));
        assert_eq!(find(CONNECTS, "address", "fd00::1"), Some(1.0));
        assert_eq!(find(CONNECTS, "port", "8080"), Some(1.0));
        assert_eq!(find(INFLIGHT, "container_id", "a"), Some(6.0));
        assert_eq!(find(ACCEPT_QUEUE, "container_id", "a"), Some(5.0));
    }

    #[test]
    fn parse_skips_unreadable_lines() {
        let samples = parse("# HELP x\nbroken\nx{a=\"1\"} nan-ish\ny 2\n");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "y");
    }
}
