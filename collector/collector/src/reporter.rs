//! Polls the eBPF maps and writes a dataset row for every process holding a GPU device.

use std::time::Duration;

use aya::Ebpf;
use collector_common::GpuActivity;
use log::{debug, info, warn};

use crate::{
    classifier::{self, Workload},
    clock::{self, NANOS_PER_MS},
    dataset::{Dataset, Sample},
    drops::DropWatcher,
    maps, process,
    syscalls::{self, RateTracker, SyscallCount, SyscallRate},
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TOP_SYSCALLS: usize = 3;

pub async fn run(ebpf: Ebpf, dataset: Dataset) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut reporter = Reporter::new(dataset);
    loop {
        interval.tick().await;
        if let Err(e) = reporter.poll(&ebpf) {
            warn!("poll eBPF maps: {e:#}");
        }
    }
}

struct Reporter {
    dataset: Dataset,
    drops: DropWatcher,
    rates: RateTracker,
}

impl Reporter {
    fn new(dataset: Dataset) -> Self {
        Self {
            dataset,
            drops: DropWatcher::default(),
            rates: RateTracker::default(),
        }
    }

    fn poll(&mut self, ebpf: &Ebpf) -> anyhow::Result<()> {
        self.drops.warn_new(ebpf)?;
        let syscall_counts = maps::syscall_counts(ebpf)?;
        let by_tgid = syscalls::group_by_tgid(syscall_counts.iter().flatten());
        let now = clock::monotonic_ns();
        for (tgid, activity) in maps::gpu_activity(ebpf)?.iter().flatten() {
            let counts = by_tgid.get(&tgid).map_or(&[][..], Vec::as_slice);
            self.report(tgid, &activity, counts, now);
        }
        Ok(())
    }

    // report writes every GPU holder: CUDA sends work through mapped queues, so a busy process can go quiet on ioctl.
    fn report(&mut self, tgid: u32, activity: &GpuActivity, counts: &[SyscallCount], now: u64) {
        let workload = classifier::classify(activity, now);
        let rates = self.rates.rates_for(tgid, counts, now);
        let process = process::read(tgid);
        let line = format!(
            "workload classified: {} tgid={tgid} comm={} ioctl_count={} top_syscalls=[{}]",
            workload.label(),
            process.comm.as_deref().unwrap_or("?"),
            activity.count,
            format_top(&rates)
        );
        if workload == Workload::GpuOnset {
            info!("{line}");
        } else {
            debug!("{line}");
        }
        self.dataset.append(&Sample {
            tgid,
            process: &process,
            activity,
            workload,
            since_last_gpu_ioctl_ms: now.saturating_sub(activity.last_ns) / NANOS_PER_MS,
            syscalls: &rates,
        });
    }
}

fn format_top(rates: &[SyscallRate]) -> String {
    rates
        .iter()
        .take(TOP_SYSCALLS)
        .map(|r| format!("{}:{:.0}/s", r.name, r.rate_per_sec))
        .collect::<Vec<_>>()
        .join(" ")
}
