//! Polls the eBPF maps and writes a dataset row for every GPU holder and every process in a watched cgroup.

use std::{collections::HashMap, time::Duration};

use aya::Ebpf;
use collector_common::GpuActivity;
use log::{debug, info, warn};

use crate::{
    inspect::{
        classifier::{self, Workload},
        framework::{Catalog, FrameworkTracker},
        process::{self, ProcessInfo},
        resources::ResourceTracker,
    },
    pipeline::{dataset::{self, Dataset, Sample}, idle::IdleThrottle, metrics::{Published, Snapshot}},
    sensing::{
        clock::{self, NANOS_PER_MS},
        drops::DropWatcher,
        maps,
        syscalls::{self, RateTracker, SyscallCount, SyscallRate},
        watch::CgroupWatch,
    },
};

// Consts
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TOP_SYSCALLS: usize = 3;
const MIB: u64 = 1024 * 1024;
const SHORT_ID_LEN: usize = 12;
const SUMMARY_POLLS: u32 = 15;
const IDLE_HEARTBEAT_POLLS: u64 = 15;
const IDLE_RATE: f64 = 0.005;
const FRAMEWORK_RECHECK_POLLS: u64 = 30;

pub async fn run(
    ebpf: Ebpf,
    dataset: Dataset,
    watch: CgroupWatch,
    frameworks: Catalog,
    published: Option<Published>,
) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut reporter = Reporter::new(dataset, watch, frameworks, published);
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
    idle: IdleThrottle,
    frameworks: FrameworkTracker,
    summary: Summary,
    published: Option<Published>,
    snapshot: Snapshot,
}

#[derive(Default)]
struct Summary {
    polls: u32,
    rows: u64,
    idle_skipped: u64,
    by_workload: [u64; 3],
}

impl Reporter {
    fn new(
        dataset: Dataset,
        watch: CgroupWatch,
        frameworks: Catalog,
        published: Option<Published>,
    ) -> Self {
        Self {
            dataset,
            watch,
            drops: DropWatcher::default(),
            rates: RateTracker::default(),
            resources: ResourceTracker::new(),
            idle: IdleThrottle::new(IDLE_HEARTBEAT_POLLS),
            frameworks: FrameworkTracker::new(frameworks, FRAMEWORK_RECHECK_POLLS),
            summary: Summary::default(),
            published,
            snapshot: Snapshot::default(),
        }
    }

    fn poll(&mut self, ebpf: &Ebpf) -> anyhow::Result<()> {
        self.drops.warn_new(ebpf)?;
        let syscall_counts = maps::syscall_counts(ebpf)?;
        let by_tgid = syscalls::group_by_tgid(syscall_counts.iter().flatten());
        let now = clock::monotonic_ns();
        for mut subject in self.subjects(ebpf)? {
            subject.process.framework = self.frameworks.framework(subject.tgid);
            let counts = by_tgid.get(&subject.tgid).map_or(&[][..], Vec::as_slice);
            self.report(&subject, counts, now);
        }
        self.publish();
        self.rates.end_poll();
        self.resources.end_poll();
        self.idle.end_poll();
        self.frameworks.end_poll();
        self.summary.end_poll();
        Ok(())
    }

    // publish swaps in this poll's snapshot; containers that left are gone from it.
    fn publish(&mut self) {
        let snapshot = std::mem::take(&mut self.snapshot);
        let Some(published) = &self.published else {
            return;
        };
        let text = snapshot.render(dataset::now_unix_ms());
        match published.write() {
            Ok(mut current) => *current = text,
            Err(poisoned) => *poisoned.into_inner() = text,
        }
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
        // The snapshot sees idle processes too: a container's rate must not drop out while its rows are thinned.
        if self.published.is_some() {
            self.snapshot.add(process, cgroup_usage.as_ref(), &rates);
        }
        // GPU holders are never thinned: a process busy on mapped GPU queues looks idle to the CPU.
        let idle = workload == Workload::NoGpu
            && usage.cpu_cores.unwrap_or(0.0) < IDLE_RATE
            && rates.iter().all(|rate| rate.rate_per_sec < IDLE_RATE);
        if !self.idle.should_write(tgid, idle) {
            self.summary.idle_skipped += 1;
            return;
        }
        let mut line = format!(
            "workload={} comm={} tgid={tgid}",
            workload.label(),
            process.comm.as_deref().unwrap_or("?"),
        );
        if let Some(framework) = &process.framework {
            line.push_str(&format!(" framework={framework}"));
        }
        if let Some(container) = &process.container_id {
            line.push_str(&format!(
                " container={}",
                &container[..SHORT_ID_LEN.min(container.len())]
            ));
        }
        line.push_str(&format!(
            " cpu={:.2} rss_mib={}",
            usage.cpu_cores.unwrap_or(0.0),
            usage.rss_bytes.unwrap_or(0) / MIB
        ));
        if let Some(cgroup) = &cgroup_usage {
            if let Some(throttled) = cgroup.cpu_throttled_fraction {
                line.push_str(&format!(" throttled={:.0}%", throttled * 100.0));
            }
            if let Some(pressure) = cgroup.cpu_pressure_avg10 {
                line.push_str(&format!(" psi={pressure:.1}"));
            }
        }
        if let Some(activity) = activity {
            line.push_str(&format!(" ioctl={}", activity.count));
        }
        if !rates.is_empty() {
            line.push_str(&format!(" syscalls={}", format_top(&rates)));
        }
        self.summary.count(workload);
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
        .join(",")
}

impl Summary {
    fn count(&mut self, workload: Workload) {
        self.rows += 1;
        self.by_workload[workload as usize] += 1;
    }

    // end_poll logs the counts since the last summary every SUMMARY_POLLS polls.
    fn end_poll(&mut self) {
        self.polls += 1;
        if self.polls < SUMMARY_POLLS {
            return;
        }
        let [onset, held, no_gpu] = self.by_workload;
        info!(
            "summary window={}s rows={} gpu-onset={onset} gpu-held={held} no-gpu={no_gpu} idle-skipped={}",
            self.polls as u64 * POLL_INTERVAL.as_secs(),
            self.rows,
            self.idle_skipped
        );
        *self = Self::default();
    }
}
