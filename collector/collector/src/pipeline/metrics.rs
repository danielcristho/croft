//! Per-container signals of the latest poll in Prometheus text format, written by the collector and read by the scaler.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, RwLock},
};

use crate::{
    inspect::{process::ProcessInfo, resources::CgroupUsage},
    sensing::syscalls::SyscallRate,
};

pub const CPU_CORES: &str = "croft_container_cpu_cores";
pub const CPU_PRESSURE: &str = "croft_container_cpu_pressure_avg10";
pub const CPU_THROTTLED: &str = "croft_container_cpu_throttled_fraction";
pub const SYSCALL_RATE: &str = "croft_container_syscall_rate";
pub const POLL_UNIX_MS: &str = "croft_poll_unix_ms";

#[derive(Debug, Default, PartialEq)]
pub struct ContainerSignals {
    pub cpu_cores: Option<f64>,
    pub cpu_pressure: Option<f64>,
    pub cpu_throttled: Option<f64>,
    pub syscalls: BTreeMap<String, f64>,
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
        cgroup: Option<&CgroupUsage>,
        rates: &[SyscallRate],
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
        for rate in rates {
            *signals.syscalls.entry(rate.name.clone()).or_default() += rate.rate_per_sec;
        }
    }

    pub fn render(&self, poll_unix_ms: u128) -> String {
        let mut text = format!("# TYPE {POLL_UNIX_MS} gauge\n{POLL_UNIX_MS} {poll_unix_ms}\n");
        let gauges: [(&str, Field); 3] = [
            (CPU_CORES, |s| s.cpu_cores),
            (CPU_PRESSURE, |s| s.cpu_pressure),
            (CPU_THROTTLED, |s| s.cpu_throttled),
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
    use super::*;

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
        snapshot.add(&process("a"), Some(&cgroup), &[rate("sendto", 10.0)]);
        snapshot.add(
            &process("a"),
            Some(&cgroup),
            &[rate("sendto", 5.0), rate("read", 1.0)],
        );
        snapshot.add(&ProcessInfo::default(), None, &[rate("sendto", 99.0)]);
        let a = &snapshot.containers["a"];
        assert_eq!(a.syscalls["sendto"], 15.0);
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
        snapshot.add(&process("abc"), Some(&cgroup), &[rate("sendto", 34.0)]);
        let samples = parse(&snapshot.render(1_700_000_000_000));
        let find = |name: &str| samples.iter().find(|s| s.name == name).map(|s| s.value);
        assert_eq!(find(POLL_UNIX_MS), Some(1.7e12));
        assert_eq!(find(CPU_CORES), Some(0.5));
        assert_eq!(find(CPU_PRESSURE), Some(12.5));
        assert_eq!(find(CPU_THROTTLED), None);
        let sendto = samples.iter().find(|s| s.name == SYSCALL_RATE).unwrap();
        assert_eq!(sendto.labels["container_id"], "abc");
        assert_eq!(sendto.labels["syscall"], "sendto");
        assert_eq!(sendto.value, 34.0);
    }

    #[test]
    fn parse_skips_unreadable_lines() {
        let samples = parse("# HELP x\nbroken\nx{a=\"1\"} nan-ish\ny 2\n");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "y");
    }
}
