// Appends one JSON Lines record per GPU-onset classification, for later labeling and training.

use std::{
    fs::OpenOptions,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use log::debug;
use serde::Serialize;

const DEFAULT_PATH: &str = "croft-dataset.jsonl";

#[derive(Serialize)]
struct SyscallRate {
    name: String,
    rate_per_sec: f64,
}

// Record is one dataset row: a feature vector plus a label left for later filling in.
#[derive(Serialize)]
struct Record {
    unix_ms: u128,
    tgid: u32,
    workload: &'static str,
    ioctl_count: u64,
    syscalls: Vec<SyscallRate>,
    label: Option<String>,
}

// append writes one record; a write failure only logs, since a dataset problem
// should not stop the collector from reporting.
pub fn append(tgid: u32, ioctl_count: u64, syscalls: Vec<(String, f64)>) {
    let record = Record {
        unix_ms: now_unix_ms(),
        tgid,
        workload: "gpu-onset",
        ioctl_count,
        syscalls: syscalls
            .into_iter()
            .map(|(name, rate_per_sec)| SyscallRate {
                name,
                rate_per_sec: round2(rate_per_sec),
            })
            .collect(),
        label: None,
    };

    if let Err(e) = write_line(&record) {
        debug!("dataset: could not append record for tgid={tgid}: {e}");
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn write_line(record: &Record) -> anyhow::Result<()> {
    let path = std::env::var("DATASET_PATH").unwrap_or_else(|_| DEFAULT_PATH.to_string());
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_valid_json_line() {
        let path = "/tmp/croft-dataset-test.jsonl";
        // SAFETY: this test runs single-threaded; no concurrent env reader to race.
        unsafe { std::env::set_var("DATASET_PATH", path) };
        let _ = std::fs::remove_file(path);

        append(
            42,
            100,
            vec![("ioctl".to_string(), 26.0), ("write".to_string(), 19.0)],
        );

        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["tgid"], 42);
        assert_eq!(parsed["workload"], "gpu-onset");
        assert_eq!(parsed["ioctl_count"], 100);
        assert_eq!(parsed["syscalls"][0]["name"], "ioctl");
        assert!(parsed["label"].is_null());

        std::fs::remove_file(path).ok();
    }
}
