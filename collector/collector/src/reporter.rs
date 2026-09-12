//! Polls GPU_ACTIVITY and reports what the rule-based classifier makes of it.

use std::time::Duration;
use aya::maps::HashMap;
use collector_common::GpuActivity;
use log::info;
use crate::{classifier, classifier::Workload, clock};

// Consts
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Runs the reporter loop.
pub async fn run(ebpf: aya::Ebpf) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    loop {
        interval.tick().await;
        let Ok(map) = HashMap::<_, u32, GpuActivity>::try_from(ebpf.map("GPU_ACTIVITY").unwrap())
        else {
            continue;
        };

        let now = clock::monotonic_ns();
        for (tgid, activity) in map.iter().flatten() {
            report(tgid, &activity, now);
        }
    }
}

// report classifies the workload and logs if it is a GPU-onset.
fn report(tgid: u32, activity: &GpuActivity, now: u64) {
    if let Workload::GpuOnset = classifier::classify(activity, now) {
        info!(
            "workload classified: gpu-onset tgid={tgid} ioctl_count={}",
            activity.count
        );
    }
}
