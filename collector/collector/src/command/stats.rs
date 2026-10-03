//! Summary of one or more datasets: size, rows, time span and rows per node, workload and label.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::{
    cli::StatsArgs,
    dataset::now_unix_ms,
    rows::{self, Line},
};

// Consts for the dataset file extension, the label for unlabelled rows, and the number of milliseconds per hour.
const DATASET_EXTENSION: &str = "jsonl";
const UNLABELLED: &str = "unlabelled";
const MS_PER_HOUR: f64 = 3_600_000.0;

pub fn execute(args: &StatsArgs) -> anyhow::Result<()> {
    let mut summary = Summary::default();
    for path in dataset_files(&args.paths)? {
        summary.add_file(&path)?;
    }
    print!("{}", summary.render(now_unix_ms() as u64));
    Ok(())
}

// dataset_files expands a directory to its *.jsonl files, as written by collect.
fn dataset_files(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if !path.is_dir() {
            files.push(path.clone());
            continue;
        }
        let mut found: Vec<PathBuf> = fs::read_dir(path)
            .with_context(|| format!("read {}", path.display()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|file| file.extension().is_some_and(|ext| ext == DATASET_EXTENSION))
            .collect();
        found.sort();
        files.extend(found);
    }
    Ok(files)
}

#[derive(Default)]
struct Summary {
    files: usize,
    bytes: u64,
    rows: u64,
    unreadable: u64,
    first_ms: Option<u64>,
    last_ms: Option<u64>,
    by_node: BTreeMap<String, u64>,
    by_workload: BTreeMap<String, u64>,
    by_label: BTreeMap<String, u64>,
}

impl Summary {
    fn add_file(&mut self, path: &Path) -> anyhow::Result<()> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        self.files += 1;
        self.bytes += file.metadata()?.len();
        let node = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        while reader.read_until(b'\n', &mut buffer)? > 0 {
            self.add_line(&node, &String::from_utf8_lossy(&buffer));
            buffer.clear();
        }
        Ok(())
    }

    fn add_line(&mut self, node: &str, line: &str) {
        let row = match rows::parse(line) {
            Line::Row(row) => row,
            Line::Blank => return,
            Line::Unreadable => {
                self.unreadable += 1;
                return;
            }
        };
        self.rows += 1;
        self.first_ms = Some(
            self.first_ms
                .map_or(row.unix_ms, |first| first.min(row.unix_ms)),
        );
        self.last_ms = Some(
            self.last_ms
                .map_or(row.unix_ms, |last| last.max(row.unix_ms)),
        );
        *self.by_node.entry(node.to_string()).or_default() += 1;
        *self.by_workload.entry(row.workload).or_default() += 1;
        *self
            .by_label
            .entry(row.label.unwrap_or_else(|| UNLABELLED.into()))
            .or_default() += 1;
    }

    fn render(&self, now_ms: u64) -> String {
        let mut out = format!(
            "files       {}\nsize        {}\nrows        {} ({} unreadable)\n",
            self.files,
            human_bytes(self.bytes),
            self.rows,
            self.unreadable
        );
        if let (Some(first), Some(last)) = (self.first_ms, self.last_ms) {
            out.push_str(&format!(
                "span        {:.1} h, newest row {}s ago\n",
                (last - first) as f64 / MS_PER_HOUR,
                now_ms.saturating_sub(last) / 1000
            ));
        }
        for (title, counts) in [
            ("node", &self.by_node),
            ("workload", &self.by_workload),
            ("label", &self.by_label),
        ] {
            out.push_str(&format!("\n{title}\n"));
            for (key, count) in counts {
                out.push_str(&format!("  {key:<24} {count:>12}\n"));
            }
        }
        out
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_count_by_node_workload_and_label() {
        let mut summary = Summary::default();
        for line in [
            r#"{"unix_ms":1000,"tgid":1,"workload":"gpu-held","label":null}"#,
            r#"{"unix_ms":4000,"tgid":2,"workload":"no-gpu","label":"web"}"#,
            r#"{"unix_ms":2000,"tgid":1,"workload":"gpu-held","label":null}"#,
            "\0\0",
            "{broken",
        ] {
            summary.add_line("node-a", line);
        }
        assert_eq!(summary.rows, 3);
        assert_eq!(summary.unreadable, 1);
        assert_eq!(
            (summary.first_ms, summary.last_ms),
            (Some(1000), Some(4000))
        );
        assert_eq!(summary.by_node["node-a"], 3);
        assert_eq!(summary.by_workload["gpu-held"], 2);
        assert_eq!(summary.by_label[UNLABELLED], 2);
        assert_eq!(summary.by_label["web"], 1);
    }

    #[test]
    fn sizes_render_in_binary_units() {
        for (bytes, text) in [
            (512, "512 B"),
            (2048, "2.0 KiB"),
            (3 * 1024 * 1024 * 1024, "3.0 GiB"),
        ] {
            assert_eq!(human_bytes(bytes), text);
        }
    }
}
