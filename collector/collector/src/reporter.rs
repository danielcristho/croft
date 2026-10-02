//! Polls the eBPF maps and writes a dataset row for every GPU holder and every process in a watched cgroup.

use std::{collections::HashMap, time::Duration};

use aya::Ebpf;
use collector_common::GpuActivity;
use log::{debug, info, warn};

use crate::{
    classifier::{self, Workload},
    clock::{self, NANOS_PER_MS},
    dataset::{Dataset, Sample},
    drops::DropWatcher,
    maps,
    process::{self, ProcessInfo},
    resources::ResourceTracker,
    syscalls::{self, RateTracker, SyscallCount, SyscallRate},
    watch::CgroupWatch,
};

// Consts
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TOP_SYSCALLS: usize = 3;
const MIB: u64 = 1024 * 1024;

pub async fn run(ebpf: Ebpf, dataset: Dataset, watch: CgroupWatch) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut reporter = Reporter::new(dataset, watch);
    loop {
        interval.tick().await;
        if let Err(e) = reporter.poll(&ebpf) {
            warn!("poll eBPF maps: {e:#}");
        }
    }
}

struct Subject {
    tgid: u32,
    process: ProcessInfo,
    activity: Option<GpuActivity>,
}

struct Reporter {
    dataset: Dataset,
    watch: CgroupWatch,
    drops: DropWatcher,
    rates: RateTracker,
    resources: ResourceTracker,
}

impl Reporter {
    fn new(dataset: Dataset, watch: CgroupWatch) -> Self {
        Self {
            dataset,
            watch,
            drops: DropWatcher::default(),
            rates: RateTracker::default(),
            resources: ResourceTracker::new(),
        }
    }

    fn poll(&mut self, ebpf: &Ebpf) -> anyhow::Result<()> {
        self.drops.warn_new(ebpf)?;
        let syscall_counts = maps::syscall_counts(ebpf)?;
        let by_tgid = syscalls::group_by_tgid(syscall_counts.iter().flatten());
        let now = clock::monotonic_ns();
        for subject in self.subjects(ebpf)? {
            let counts = by_tgid.get(&subject.tgid).map_or(&[][..], Vec::as_slice);
            self.report(&subject, counts, now);
        }
        self.rates.end_poll();
        self.resources.end_poll();
        Ok(())
    }

    // subjects adds, to the GPU holders, every process in a watched cgroup except pod sandboxes.
    fn subjects(&self, ebpf: &Ebpf) -> anyhow::Result<Vec<Subject>> {
        let holders: HashMap<u32, GpuActivity> =
            maps::gpu_activity(ebpf)?.iter().flatten().collect();
        let mut subjects: Vec<Subject> = holders
            .iter()
            .map(|(&tgid, &activity)| Subject {
                tgid,
                process: process::read(tgid),
                activity: Some(activity),
            })
            .collect();
        if self.watch.is_empty() {
            return Ok(subjects);
        }
        let own = std::process::id();
        subjects.extend(
            self.watch
                .processes()
                .into_iter()
                .filter(|(tgid, _)| *tgid != own && !holders.contains_key(tgid))
                .map(|(tgid, cgroup)| Subject {
                    tgid,
                    process: process::identify(tgid, Some(cgroup)),
                    activity: None,
                })
                .filter(|subject| !subject.process.is_pod_sandbox()),
        );
        Ok(subjects)
    }

    // report writes every GPU holder: CUDA sends work through mapped queues, so a busy process can go quiet on ioctl.
    fn report(&mut self, subject: &Subject, counts: &[SyscallCount], now: u64) {
        let Subject {
            tgid,
            process,
            activity,
        } = subject;
        let tgid = *tgid;
        let workload = classifier::classify(activity.as_ref(), now);
        let rates = self.rates.rates_for(tgid, counts, now);
        let usage = self.resources.process(tgid, now);
        let cgroup_usage = process
            .cgroup
            .as_deref()
            .map(|cgroup| self.resources.cgroup(cgroup, now));
        let line = format!(
            "workload classified: {} tgid={tgid} comm={} cpu_cores={:.2} rss_mib={} ioctl_count={} top_syscalls=[{}]",
            workload.label(),
            process.comm.as_deref().unwrap_or("?"),
            usage.cpu_cores.unwrap_or(0.0),
            usage.rss_bytes.unwrap_or(0) / MIB,
            activity.map_or(0, |activity| activity.count),
            format_top(&rates)
        );
        if workload == Workload::GpuOnset {
            info!("{line}");
        } else {
            debug!("{line}");
        }
        self.dataset.append(&Sample {
            tgid,
            process,
            activity: activity.as_ref(),
            workload,
            since_last_gpu_ioctl_ms: activity
                .map(|activity| now.saturating_sub(activity.last_ns) / NANOS_PER_MS),
            usage: &usage,
            cgroup_usage: cgroup_usage.as_ref(),
            syscalls: &rates,
        });
    }
}

// format_top returns a string of the top N syscalls and their rates, e.g. "ioctl:123/s read:45/s write:6/s".
fn format_top(rates: &[SyscallRate]) -> String {
    rates
        .iter()
        .take(TOP_SYSCALLS)
        .map(|r| format!("{}:{:.0}/s", r.name, r.rate_per_sec))
        .collect::<Vec<_>>()
        .join(" ")
}
