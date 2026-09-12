// Polls GPU_ACTIVITY and SYSCALL_BY_TYPE, and reports what the classifier makes of them.

use std::{collections::HashMap as StdHashMap, time::Duration};

use aya::maps::HashMap;
use collector_common::GpuActivity;
use log::info;

use crate::{
    classifier::{self, Workload},
    clock, syscalls,
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TOP_SYSCALLS: usize = 3;

// run polls both maps every POLL_INTERVAL and reports each GPU-onset workload.
pub async fn run(ebpf: aya::Ebpf) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut rates = syscalls::RateTracker::default();

    loop {
        interval.tick().await;
        let Ok(gpu_activity) =
            HashMap::<_, u32, GpuActivity>::try_from(ebpf.map("GPU_ACTIVITY").unwrap())
        else {
            continue;
        };
        let Ok(syscall_counts) =
            HashMap::<_, u64, u64>::try_from(ebpf.map("SYSCALL_BY_TYPE").unwrap())
        else {
            continue;
        };

        // Group the flat (tgid, syscall_nr) -> count map by tgid.
        let mut by_tgid: StdHashMap<u32, Vec<(i64, u64)>> = StdHashMap::new();
        for (key, count) in syscall_counts.iter().flatten() {
            let (tgid, syscall_nr) = syscalls::unpack_key(key);
            by_tgid.entry(tgid).or_default().push((syscall_nr, count));
        }

        let now = clock::monotonic_ns();
        for (tgid, activity) in gpu_activity.iter().flatten() {
            report(tgid, &activity, now, by_tgid.get(&tgid), &mut rates);
        }
    }
}

// report logs a GPU-onset classification; idle workloads stay silent.
fn report(
    tgid: u32,
    activity: &GpuActivity,
    now: u64,
    tgid_syscalls: Option<&Vec<(i64, u64)>>,
    rates: &mut syscalls::RateTracker,
) {
    if !matches!(classifier::classify(activity, now), Workload::GpuOnset) {
        return;
    }

    let top = top_syscalls_by_rate(tgid, tgid_syscalls, now, rates);
    info!(
        "workload classified: gpu-onset tgid={tgid} ioctl_count={} top_syscalls=[{top}]",
        activity.count
    );
}

// top_syscalls_by_rate renders a tgid's busiest syscalls by name, capped to TOP_SYSCALLS.
fn top_syscalls_by_rate(
    tgid: u32,
    tgid_syscalls: Option<&Vec<(i64, u64)>>,
    now: u64,
    rates: &mut syscalls::RateTracker,
) -> String {
    let Some(tgid_syscalls) = tgid_syscalls else {
        return String::new();
    };

    let mut rated: Vec<(String, f64)> = tgid_syscalls
        .iter()
        .map(|&(nr, count)| (syscalls::name(nr), rates.rate_per_sec(tgid, nr, count, now)))
        .collect();
    rated.sort_by(|a, b| b.1.total_cmp(&a.1));
    rated.truncate(TOP_SYSCALLS);

    rated
        .into_iter()
        .map(|(name, rate)| format!("{name}:{rate:.0}/s"))
        .collect::<Vec<_>>()
        .join(" ")
}
