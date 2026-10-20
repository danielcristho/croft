//! JSON Lines dataset, one row per reported process per poll, for labelling and training.

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use collector_common::GpuActivity;
use log::{info, warn};
use serde::Serialize;

use crate::{
    inspect::{
        classifier::Workload,
        process::ProcessInfo,
        resources::{CgroupUsage, ProcessUsage},
    },
    pipeline::{
        rows::{self, Line},
        segments::{self, Compressor, Policy},
    },
    sensing::syscalls::SyscallRate,
};

// Consts for dataset path and default filename, and how many leading lines may lack a row.
pub const PATH_ENV: &str = "DATASET_PATH";
pub const DEFAULT_PATH: &str = "/var/lib/croft/croft-dataset.jsonl";
const FIRST_ROW_LINES: usize = 10;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    ioctl_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    since_last_gpu_ioctl_ms: Option<u64>,
    usage: &'a ProcessUsage,
    cgroup_usage: Option<&'a CgroupUsage>,
    syscalls: Vec<SyscallRate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

pub struct Dataset {
    path: PathBuf,
    rotation: Option<Rotation>,
}

struct Rotation {
    policy: Policy,
    stem: String,
    started_ms: u64,
    compressor: Compressor,
}

impl Dataset {
    // new writes one file forever, without rotation.
    #[cfg(test)]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            rotation: None,
        }
    }

    // with_rotation dates the live file by its first row, so a file left by an older collector rotates on the first poll.
    pub fn with_rotation(path: impl Into<PathBuf>, policy: Policy) -> Self {
        let path = path.into();
        let stem = stem(&path);
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let started_ms = first_row_ms(&path).unwrap_or_else(|| now_unix_ms() as u64);
        let compressor = Compressor::start(dir, stem.clone(), policy);
        Self {
            path,
            rotation: Some(Rotation {
                policy,
                stem,
                started_ms,
                compressor,
            }),
        }
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

    // end_poll rotates the live file once it is due, between polls, so one poll's rows stay in one segment.
    pub fn end_poll(&mut self) {
        let Some(rotation) = &mut self.rotation else {
            return;
        };
        let Ok(meta) = fs::metadata(&self.path) else {
            return;
        };
        let now = now_unix_ms() as u64;
        let age = Duration::from_millis(now.saturating_sub(rotation.started_ms));
        if !rotation.policy.is_due(age, meta.len()) {
            return;
        }
        match segments::rotate(&self.path, &rotation.stem, rotation.started_ms) {
            Ok(segment) => {
                info!("rotated {}", segment.display());
                rotation.compressor.submit(segment);
                rotation.started_ms = now;
            }
            Err(e) => warn!("rotate {}: {e}", self.path.display()),
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

// stem is the live file's name without .jsonl; segments are named after it.
fn stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    name.strip_suffix(segments::LIVE_SUFFIX)
        .map_or(name.clone(), str::to_string)
}

// first_row_ms reads the time of the live file's first row, or None when it is missing or empty.
fn first_row_ms(path: &Path) -> Option<u64> {
    let reader = segments::open(path).ok()?;
    reader
        .lines()
        .take(FIRST_ROW_LINES)
        .map_while(Result::ok)
        .find_map(|line| match rows::parse(&line) {
            Line::Row(row) => Some(row.unix_ms),
            Line::Blank | Line::Unreadable => None,
        })
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
    use std::io::Read;

    use super::*;

    #[test]
    fn live_file_rotates_once_due_and_is_compressed() {
        let dir =
            std::env::temp_dir().join(format!("croft-dataset-rotation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let live = dir.join("croft-dataset.jsonl");
        fs::write(
            &live,
            "{\"unix_ms\":1792478649836,\"tgid\":1,\"workload\":\"no-gpu\"}\n",
        )
        .unwrap();
        let policy = Policy {
            rotate_bytes: Some(1),
            ..Policy::default()
        };

        let mut dataset = Dataset::with_rotation(&live, policy);
        dataset.end_poll();

        assert!(!live.exists());
        let compressed = dir.join("croft-dataset-20261020T064409Z.jsonl.zst");
        for _ in 0..100 {
            if compressed.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut text = String::new();
        segments::open(&compressed)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.starts_with("{\"unix_ms\":1792478649836"));
        fs::remove_dir_all(&dir).unwrap();
    }

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
