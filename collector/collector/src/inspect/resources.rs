//! CPU, memory, I/O and network of a process and its cgroup, from /proc and cgroup v2.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{inspect::process::CGROUP_ROOT, pipeline::delta::DeltaTracker};

// Consts for the network namespace files in /proc.
const HOST_NETNS: &str = "/proc/1/ns/net";
const LOOPBACK: &str = "lo";
const KIB: u64 = 1024;
const USEC_PER_SEC: f64 = 1_000_000.0;
const FALLBACK_TICKS_PER_SEC: f64 = 100.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessUsage {
    pub cpu_cores: Option<f64>,
    pub rss_bytes: Option<u64>,
    pub threads: Option<u64>,
    pub ctx_voluntary_s: Option<f64>,
    pub ctx_involuntary_s: Option<f64>,
    pub read_bytes_s: Option<f64>,
    pub write_bytes_s: Option<f64>,
    pub network_rx_bytes_s: Option<f64>,
    pub network_tx_bytes_s: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CgroupUsage {
    pub cpu_cores: Option<f64>,
    pub cpu_throttled_fraction: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub oom_kills: Option<u64>,
    pub cpu_pressure_avg10: Option<f64>,
    pub memory_pressure_avg10: Option<f64>,
    pub io_pressure_avg10: Option<f64>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Counter {
    CpuTicks,
    CtxVoluntary,
    CtxInvoluntary,
    ReadBytes,
    WriteBytes,
    NetworkRx,
    NetworkTx,
    CgroupCpuUsec,
    CgroupThrottledUsec,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum Owner {
    Process(u32),
    Cgroup(String),
}

pub struct ResourceTracker {
    deltas: DeltaTracker<(Owner, Counter)>,
    // Processes of one container share a cgroup; each is read once per poll.
    cgroups: HashMap<String, CgroupUsage>,
    ticks_per_sec: f64,
    host_netns: Option<PathBuf>,
}

impl ResourceTracker {
    pub fn new() -> Self {
        Self {
            deltas: DeltaTracker::default(),
            cgroups: HashMap::new(),
            ticks_per_sec: clock_ticks_per_sec(),
            host_netns: fs::read_link(HOST_NETNS).ok(),
        }
    }

    pub fn process(&mut self, tgid: u32, now_ns: u64) -> ProcessUsage {
        let dir = PathBuf::from(format!("/proc/{tgid}"));
        let status = read_fields(&dir.join("status"));
        let io = read_fields(&dir.join("io"));
        let ticks = read(&dir.join("stat")).and_then(|stat| cpu_ticks(&stat));
        let network = self.own_network(&dir);
        let owner = Owner::Process(tgid);
        ProcessUsage {
            cpu_cores: self
                .rate(&owner, Counter::CpuTicks, ticks, now_ns)
                .map(|ticks_s| ticks_s / self.ticks_per_sec),
            rss_bytes: status.get("VmRSS").map(|kib| kib * KIB),
            threads: status.get("Threads").copied(),
            ctx_voluntary_s: self.rate(
                &owner,
                Counter::CtxVoluntary,
                status.get("voluntary_ctxt_switches").copied(),
                now_ns,
            ),
            ctx_involuntary_s: self.rate(
                &owner,
                Counter::CtxInvoluntary,
                status.get("nonvoluntary_ctxt_switches").copied(),
                now_ns,
            ),
            read_bytes_s: self.rate(
                &owner,
                Counter::ReadBytes,
                io.get("read_bytes").copied(),
                now_ns,
            ),
            write_bytes_s: self.rate(
                &owner,
                Counter::WriteBytes,
                io.get("write_bytes").copied(),
                now_ns,
            ),
            network_rx_bytes_s: self.rate(
                &owner,
                Counter::NetworkRx,
                network.map(|(rx, _)| rx),
                now_ns,
            ),
            network_tx_bytes_s: self.rate(
                &owner,
                Counter::NetworkTx,
                network.map(|(_, tx)| tx),
                now_ns,
            ),
        }
    }

    pub fn cgroup(&mut self, cgroup: &str, now_ns: u64) -> CgroupUsage {
        if let Some(usage) = self.cgroups.get(cgroup) {
            return usage.clone();
        }
        let dir = Path::new(CGROUP_ROOT).join(cgroup.trim_start_matches('/'));
        let cpu = read_fields(&dir.join("cpu.stat"));
        let memory = read_fields(&dir.join("memory.stat"));
        let owner = Owner::Cgroup(cgroup.to_string());
        let usage = CgroupUsage {
            cpu_cores: self
                .rate(
                    &owner,
                    Counter::CgroupCpuUsec,
                    cpu.get("usage_usec").copied(),
                    now_ns,
                )
                .map(|usec_s| usec_s / USEC_PER_SEC),
            cpu_throttled_fraction: self
                .rate(
                    &owner,
                    Counter::CgroupThrottledUsec,
                    cpu.get("throttled_usec").copied(),
                    now_ns,
                )
                .map(|usec_s| usec_s / USEC_PER_SEC),
            memory_bytes: read(&dir.join("memory.current"))
                .and_then(|current| current.trim().parse::<u64>().ok())
                .map(|current| {
                    current.saturating_sub(memory.get("inactive_file").copied().unwrap_or(0))
                }),
            oom_kills: read_fields(&dir.join("memory.events"))
                .get("oom_kill")
                .copied(),
            cpu_pressure_avg10: read(&dir.join("cpu.pressure")).and_then(|p| some_avg10(&p)),
            memory_pressure_avg10: read(&dir.join("memory.pressure")).and_then(|p| some_avg10(&p)),
            io_pressure_avg10: read(&dir.join("io.pressure")).and_then(|p| some_avg10(&p)),
        };
        self.cgroups.insert(cgroup.to_string(), usage.clone());
        usage
    }

    pub fn end_poll(&mut self) {
        self.cgroups.clear();
        self.deltas.end_poll();
    }

    fn rate(
        &mut self,
        owner: &Owner,
        counter: Counter,
        value: Option<u64>,
        now_ns: u64,
    ) -> Option<f64> {
        self.deltas
            .per_sec((owner.clone(), counter), value?, now_ns)
    }

    // own_network skips processes on the host network, whose counters are the whole machine's traffic.
    fn own_network(&self, dir: &Path) -> Option<(u64, u64)> {
        let netns = fs::read_link(dir.join("ns/net")).ok()?;
        if Some(&netns) == self.host_netns.as_ref() {
            return None;
        }
        read(&dir.join("net/dev")).map(|dev| network_totals(&dev))
    }
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn read_fields(path: &Path) -> HashMap<String, u64> {
    read(path)
        .map(|content| fields(&content))
        .unwrap_or_default()
}

// fields parses "key: value [unit]" and "key value" lines, the shapes of /proc status/io and cgroup stat files.
fn fields(content: &str) -> HashMap<String, u64> {
    content
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(|c: char| c == ':' || c.is_whitespace())?;
            let value = rest.split_whitespace().next()?.parse().ok()?;
            Some((key.to_string(), value))
        })
        .collect()
}

// cpu_ticks sums utime and stime, fields 14 and 15 of /proc/<pid>/stat, counted after the comm in parentheses.
fn cpu_ticks(stat: &str) -> Option<u64> {
    let (_, after_comm) = stat.rsplit_once(')')?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn some_avg10(pressure: &str) -> Option<f64> {
    pressure
        .lines()
        .find(|line| line.starts_with("some "))?
        .split_whitespace()
        .find_map(|field| field.strip_prefix("avg10="))?
        .parse()
        .ok()
}

// network_totals sums received and sent bytes over every interface but loopback.
fn network_totals(dev: &str) -> (u64, u64) {
    dev.lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim() != LOOPBACK)
        .filter_map(|(_, counters)| {
            let counters: Vec<u64> = counters
                .split_whitespace()
                .filter_map(|c| c.parse().ok())
                .collect();
            Some((*counters.first()?, *counters.get(8)?))
        })
        .fold((0, 0), |(rx, tx), (r, t)| (rx + r, tx + t))
}

fn clock_ticks_per_sec() -> f64 {
    // SAFETY: sysconf reads a constant and has no preconditions.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 {
        ticks as f64
    } else {
        FALLBACK_TICKS_PER_SEC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_proc_and_cgroup_shapes() {
        let status = "Threads:\t15\nVmRSS:\t  310120 kB\nName:\tgnome-shell\n";
        let parsed = fields(status);
        assert_eq!(parsed["Threads"], 15);
        assert_eq!(parsed["VmRSS"], 310120);
        assert!(!parsed.contains_key("Name"));
        assert_eq!(
            fields("usage_usec 3667083\nnr_periods 0\n")["usage_usec"],
            3667083
        );
    }

    #[test]
    fn cpu_ticks_skips_a_comm_with_spaces_and_parens() {
        let stat = "42 (my (odd) proc) S 1 1 1 0 1 4194304 50275 0 0 0 158 48 0 0 20 0 15 0";
        assert_eq!(cpu_ticks(stat), Some(206));
    }

    #[test]
    fn some_avg10_reads_the_some_line() {
        let pressure = "some avg10=1.25 avg60=0.50 avg300=0.10 total=13874\nfull avg10=0.75 avg60=0.00 avg300=0.00 total=10984\n";
        assert_eq!(some_avg10(pressure), Some(1.25));
        assert_eq!(some_avg10(""), None);
    }

    #[test]
    fn network_totals_skip_loopback() {
        let dev = "Inter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n";
        assert_eq!(network_totals(dev), (1000, 2000));
    }

    #[test]
    fn this_process_reads_back() {
        let mut tracker = ResourceTracker::new();
        let usage = tracker.process(std::process::id(), 0);
        assert!(usage.rss_bytes.is_some_and(|rss| rss > 0));
        assert!(usage.threads.is_some_and(|threads| threads >= 1));
        assert_eq!(usage.cpu_cores, None);
    }
}
