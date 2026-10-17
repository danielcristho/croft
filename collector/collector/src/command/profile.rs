//! Resource profile of one or more containers over a window, as JSON, for allocation decisions.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use anyhow::{Context, bail};
use serde::Serialize;

use super::stats::dataset_files;
use crate::{
    cli::ProfileArgs,
    pipeline::rows::{self, Line, Row},
};

// Const for the poll interval in milliseconds, the GPU workloads to check, and the percentile for the summary statistics.
const POLL_MS: u64 = 2_000;
const GPU_WORKLOADS: [&str; 2] = ["gpu-onset", "gpu-held"];
const PERCENTILE: f64 = 0.95;

// execute profiles the container with the given prefix in the dataset files, summarizing its resource usage over the given time window, and prints the result as JSON.
pub fn execute(args: &ProfileArgs) -> anyhow::Result<()> {
    let prefix = args.container.to_ascii_lowercase();
    if prefix.len() < 4 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("container must be at least 4 hex digits of a container id");
    }
    let mut polls = Polls::default();
    for path in dataset_files(&args.paths)? {
        polls.read(&path, &prefix, args.from_ms, args.to_ms)?;
    }
    let profiles: Vec<Profile> = polls
        .by_container
        .iter()
        .map(|(id, p)| p.summarize(id))
        .collect();
    if profiles.is_empty() {
        bail!("no rows for a container starting with {prefix}");
    }
    println!("{}", serde_json::to_string_pretty(&profiles)?);
    Ok(())
}

// Sample is one poll of one container: its cgroup's numbers, or the sum of its processes where no cgroup was read.
#[derive(Default)]
struct Sample {
    cpu_cores: Option<f64>,
    memory_bytes: Option<f64>,
    throttled_fraction: Option<f64>,
    cpu_pressure: Option<f64>,
    process_cpu: f64,
    process_rss: f64,
    holds_gpu: bool,
    tgids: Vec<u32>,
}

#[derive(Default)]
struct ContainerPolls {
    nodes: Vec<String>,
    samples: BTreeMap<u64, Sample>,
}

#[derive(Default)]
struct Polls {
    by_container: BTreeMap<String, ContainerPolls>,
}

impl Polls {
    fn read(
        &mut self,
        path: &Path,
        prefix: &str,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> anyhow::Result<()> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let node = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let needle = format!("\"container_id\":\"{prefix}");
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        while reader.read_until(b'\n', &mut buffer)? > 0 {
            // Only lines naming the container are parsed; a dataset holds gigabytes of other rows.
            if contains(&buffer, &needle)
                && let Line::Row(row) = rows::parse(&String::from_utf8_lossy(&buffer))
                && from_ms.is_none_or(|from| row.unix_ms >= from)
                && to_ms.is_none_or(|to| row.unix_ms <= to)
            {
                self.add(&node, &row);
            }
            buffer.clear();
        }
        Ok(())
    }

    fn add(&mut self, node: &str, row: &Row) {
        let Some(id) = row.process.container_id.clone() else {
            return;
        };
        let container = self.by_container.entry(id).or_default();
        if !container.nodes.iter().any(|known| known == node) {
            container.nodes.push(node.to_string());
        }
        let sample = container.samples.entry(row.unix_ms / POLL_MS).or_default();
        if let Some(cgroup) = &row.cgroup_usage {
            sample.cpu_cores = cgroup.cpu_cores.or(sample.cpu_cores);
            sample.memory_bytes = cgroup
                .memory_bytes
                .map(|bytes| bytes as f64)
                .or(sample.memory_bytes);
            sample.throttled_fraction = cgroup.cpu_throttled_fraction.or(sample.throttled_fraction);
            sample.cpu_pressure = cgroup.cpu_pressure_avg10.or(sample.cpu_pressure);
        }
        sample.process_cpu += row.usage.cpu_cores.unwrap_or(0.0);
        sample.process_rss += row.usage.rss_bytes.unwrap_or(0) as f64;
        sample.holds_gpu |= GPU_WORKLOADS.contains(&row.workload.as_str());
        if !sample.tgids.contains(&row.tgid) {
            sample.tgids.push(row.tgid);
        }
    }
}

#[derive(Debug, Serialize)]
struct Stat {
    mean: f64,
    p95: f64,
    max: f64,
}

#[derive(Debug, Serialize)]
struct Profile {
    container_id: String,
    nodes: Vec<String>,
    from_ms: u64,
    to_ms: u64,
    polls: usize,
    processes: usize,
    cpu_cores: Option<Stat>,
    memory_bytes: Option<Stat>,
    gpu_share: f64,
    throttled_fraction: Option<Stat>,
    cpu_pressure: Option<Stat>,
}

impl ContainerPolls {
    fn summarize(&self, id: &str) -> Profile {
        let samples: Vec<&Sample> = self.samples.values().collect();
        let first = self.samples.keys().next().copied().unwrap_or(0);
        let last = self.samples.keys().last().copied().unwrap_or(0);
        let mut tgids: Vec<u32> = samples
            .iter()
            .flat_map(|s| s.tgids.iter().copied())
            .collect();
        tgids.sort_unstable();
        tgids.dedup();
        let held = samples.iter().filter(|s| s.holds_gpu).count();
        Profile {
            container_id: id.to_string(),
            nodes: self.nodes.clone(),
            from_ms: first * POLL_MS,
            to_ms: last * POLL_MS,
            polls: samples.len(),
            processes: tgids.len(),
            cpu_cores: stat(samples.iter().map(|s| s.cpu_cores.or(Some(s.process_cpu)))),
            memory_bytes: stat(
                samples
                    .iter()
                    .map(|s| s.memory_bytes.or(Some(s.process_rss))),
            ),
            gpu_share: if samples.is_empty() {
                0.0
            } else {
                held as f64 / samples.len() as f64
            },
            throttled_fraction: stat(samples.iter().map(|s| s.throttled_fraction)),
            cpu_pressure: stat(samples.iter().map(|s| s.cpu_pressure)),
        }
    }
}

fn stat(values: impl Iterator<Item = Option<f64>>) -> Option<Stat> {
    let mut values: Vec<f64> = values.flatten().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * PERCENTILE).ceil() as usize).clamp(1, values.len()) - 1;
    Some(Stat {
        mean: values.iter().sum::<f64>() / values.len() as f64,
        p95: values[index],
        max: values[values.len() - 1],
    })
}

// contains uses str search, far faster than comparing every window; a line that is not UTF-8 is not a row.
fn contains(haystack: &[u8], needle: &str) -> bool {
    std::str::from_utf8(haystack).is_ok_and(|line| line.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "3aa671fa5cf11e654dbb669e4e7236ceef898b9fe90f2c5251a02757e216bf5f";

    fn line(unix_ms: u64, tgid: u32, workload: &str, cpu: f64, throttled: f64) -> String {
        format!(
            r#"{{"unix_ms":{unix_ms},"tgid":{tgid},"process":{{"comm":"p","exe":null,"cgroup":null,"container_id":"{ID}"}},"workload":"{workload}","usage":{{"cpu_cores":0.1,"rss_bytes":1000}},"cgroup_usage":{{"cpu_cores":{cpu},"cpu_throttled_fraction":{throttled},"memory_bytes":2048}},"syscalls":[],"label":null}}"#
        )
    }

    fn polls(lines: &[String]) -> ContainerPolls {
        let dir = std::env::temp_dir().join(format!("croft-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("node-a.jsonl");
        std::fs::write(
            &path,
            lines.join("\n")
                + "\n{\"unix_ms\":1,\"tgid\":9,\"workload\":\"no-gpu\",\"label\":null}\n",
        )
        .unwrap();
        let mut polls = Polls::default();
        polls.read(&path, "3aa671", None, None).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        polls.by_container.remove(ID).unwrap()
    }

    #[test]
    fn container_polls_take_the_cgroup_once_per_poll() {
        let container = polls(&[
            line(10_000, 1, "no-gpu", 0.5, 0.1),
            line(10_010, 2, "no-gpu", 0.5, 0.1),
            line(12_000, 1, "gpu-held", 1.0, 0.3),
        ]);
        let profile = container.summarize(ID);
        assert_eq!(profile.nodes, ["node-a"]);
        assert_eq!((profile.polls, profile.processes), (2, 2));
        let cpu = profile.cpu_cores.unwrap();
        assert_eq!((cpu.mean, cpu.max), (0.75, 1.0));
        assert_eq!(profile.gpu_share, 0.5);
        assert_eq!(profile.memory_bytes.unwrap().max, 2048.0);
    }

    #[test]
    fn stats_take_the_nearest_rank_percentile() {
        let values = (1..=20).map(|v| Some(v as f64));
        let summary = stat(values).unwrap();
        assert_eq!((summary.mean, summary.p95, summary.max), (10.5, 19.0, 20.0));
        assert!(stat([None, None].into_iter()).is_none());
    }
}
