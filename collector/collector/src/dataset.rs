//! JSON Lines dataset, one row per reported process per poll, for labelling and training.

use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use collector_common::GpuActivity;
use log::warn;
use serde::Serialize;

use crate::{
    classifier::Workload,
    process::ProcessInfo,
    resources::{CgroupUsage, ProcessUsage},
    syscalls::SyscallRate,
};

// Consts for dataset path and default filename.
pub const PATH_ENV: &str = "DATASET_PATH";
pub const DEFAULT_PATH: &str = "/var/lib/croft/croft-dataset.jsonl";

pub struct Sample<'a> {
    pub tgid: u32,
    pub process: &'a ProcessInfo,
    pub activity: Option<&'a GpuActivity>,
    pub workload: Workload,
    pub since_last_gpu_ioctl_ms: Option<u64>,
    pub usage: &'a ProcessUsage,
    pub cgroup_usage: Option<&'a CgroupUsage>,
    pub syscalls: &'a [SyscallRate],
}

#[derive(Serialize)]
struct Record<'a> {
    unix_ms: u128,
    tgid: u32,
    process: &'a ProcessInfo,
    workload: &'static str,
    ioctl_count: Option<u64>,
    since_last_gpu_ioctl_ms: Option<u64>,
    usage: &'a ProcessUsage,
    cgroup_usage: Option<&'a CgroupUsage>,
    syscalls: Vec<SyscallRate>,
    label: Option<String>,
}

pub struct Dataset {
    path: PathBuf,
}

impl Dataset {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    // append logs a write error instead of returning it, so a dataset problem does not stop reporting.
    pub fn append(&self, sample: &Sample) {
        let record = Record {
            unix_ms: now_unix_ms(),
            tgid: sample.tgid,
            process: sample.process,
            workload: sample.workload.label(),
            ioctl_count: sample.activity.map(|activity| activity.count),
            since_last_gpu_ioctl_ms: sample.since_last_gpu_ioctl_ms,
            usage: sample.usage,
            cgroup_usage: sample.cgroup_usage,
            syscalls: active(sample.syscalls),
            label: None,
        };
        if let Err(e) = self.write_line(&record) {
            warn!("append dataset record for tgid={}: {e}", sample.tgid);
        }
    }

    fn write_line(&self, record: &Record) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(record)?)?;
        Ok(())
    }
}

// active leaves out syscalls whose rounded rate is zero; a missing name reads as zero.
fn active(rates: &[SyscallRate]) -> Vec<SyscallRate> {
    rates
        .iter()
        .map(rounded)
        .filter(|rate| rate.rate_per_sec > 0.0)
        .collect()
}

// rounded rounds a syscall rate to two decimal places, so that a very low rate is treated as zero.
fn rounded(rate: &SyscallRate) -> SyscallRate {
    SyscallRate {
        name: rate.name.clone(),
        rate_per_sec: (rate.rate_per_sec * 100.0).round() / 100.0,
    }
}

// now_unix_ms returns the current time in ms since the Unix epoch, or zero on error.
pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_valid_json_line() {
        let path = std::env::temp_dir().join(format!("croft-dataset-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let activity = GpuActivity {
            count: 100,
            last_ns: 0,
        };
        let syscalls = [
            SyscallRate {
                name: "ioctl".into(),
                rate_per_sec: 26.004,
            },
            SyscallRate {
                name: "futex".into(),
                rate_per_sec: 0.001,
            },
        ];

        let process = ProcessInfo {
            comm: Some("python3".into()),
            ..ProcessInfo::default()
        };

        Dataset::new(&path).append(&Sample {
            tgid: 42,
            process: &process,
            activity: Some(&activity),
            workload: Workload::GpuOnset,
            since_last_gpu_ioctl_ms: Some(1500),
            usage: &ProcessUsage {
                cpu_cores: Some(1.5),
                ..ProcessUsage::default()
            },
            cgroup_usage: None,
            syscalls: &syscalls,
        });

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["tgid"], 42);
        assert_eq!(parsed["process"]["comm"], "python3");
        assert!(parsed["process"]["container_id"].is_null());
        assert_eq!(parsed["workload"], "gpu-onset");
        assert_eq!(parsed["ioctl_count"], 100);
        assert_eq!(parsed["since_last_gpu_ioctl_ms"], 1500);
        assert_eq!(parsed["usage"]["cpu_cores"], 1.5);
        assert!(parsed["cgroup_usage"].is_null());
        assert_eq!(parsed["syscalls"][0]["name"], "ioctl");
        assert_eq!(parsed["syscalls"][0]["rate_per_sec"], 26.0);
        assert_eq!(parsed["syscalls"].as_array().unwrap().len(), 1);
        assert!(parsed["label"].is_null());
        std::fs::remove_file(&path).ok();
    }
}
