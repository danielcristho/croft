//! Live table of the processes in a dataset's newest poll.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    thread,
    time::Duration,
};

use anyhow::Context;

use crate::{
    cli::TopArgs,
    dataset::now_unix_ms,
    rows::{self, Line, Row},
};

// Consts for the number of bytes to read from the end of the dataset file, the time window for a poll, and the number of top syscalls to display.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;
const POLL_SPREAD_MS: u64 = 1_000;
const TOP_SYSCALLS: usize = 3;
const SHORT_ID_LEN: usize = 12;
const MIB: f64 = 1024.0 * 1024.0;
const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

// execute runs the top command, printing the newest poll of the dataset every interval seconds until interrupted.
pub fn execute(args: &TopArgs) -> anyhow::Result<()> {
    let path = &args.dataset.dataset;
    loop {
        let poll = newest_poll(read_tail(path)?);
        if !args.once {
            print!("{CLEAR_SCREEN}");
        }
        print!("{}", render(path, &poll, now_unix_ms() as u64));
        if args.once {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(args.interval));
    }
}

// read_tail reads the last TAIL_BYTES of the dataset file and returns the rows it contains.
fn read_tail(path: &Path) -> anyhow::Result<Vec<Row>> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let start = file.metadata()?.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let lines = text.lines().skip(usize::from(start > 0));
    Ok(lines
        .filter_map(|line| match rows::parse(line) {
            Line::Row(row) => Some(*row),
            Line::Blank | Line::Unreadable => None,
        })
        .collect())
}

// newest_poll keeps the rows written by the last poll, busiest CPU first.
fn newest_poll(rows: Vec<Row>) -> Vec<Row> {
    let Some(newest) = rows.iter().map(|row| row.unix_ms).max() else {
        return rows;
    };
    let mut poll: Vec<Row> = rows
        .into_iter()
        .filter(|row| row.unix_ms + POLL_SPREAD_MS > newest)
        .collect();
    poll.sort_by(|a, b| cpu(b).total_cmp(&cpu(a)));
    poll
}

fn cpu(row: &Row) -> f64 {
    row.usage.cpu_cores.unwrap_or(0.0)
}

fn render(path: &Path, poll: &[Row], now_ms: u64) -> String {
    let age = poll.first().map_or_else(
        || "no rows".to_string(),
        |row| {
            format!(
                "newest row {}s ago",
                now_ms.saturating_sub(row.unix_ms) / 1000
            )
        },
    );
    let mut out = format!(
        "croft top  {}  {} processes  {age}\n\n",
        path.display(),const TAIL_BYTES: u64 = 4 * 1024 * 1024;

        poll.len()
    );
    out.push_str(&format!(
        "{:>8} {:<15} {:<12} {:<9} {:>5} {:>8} {:>6} {:>6}  {}\n",
        "TGID",
        "COMM",
        "CONTAINER",
        "WORKLOAD",
        "CPU",
        "RSS_MIB",
        "THROT%",
        "PSI%",
        "TOP SYSCALLS/S"
    ));
    for row in poll {
        out.push_str(&line(row));
        out.push('\n');
    }
    out
}

fn line(row: &Row) -> String {
    let cgroup = row.cgroup_usage.as_ref();
    format!(
        "{:>8} {:<15} {:<12} {:<9} {:>5} {:>8} {:>6} {:>6}  {}",
        row.tgid,
        truncate(row.process.comm.as_deref().unwrap_or("?"), 15),
        truncate(
            row.process.container_id.as_deref().unwrap_or("-"),
            SHORT_ID_LEN
        ),
        row.workload,
        number(row.usage.cpu_cores, 2),
        number(row.usage.rss_bytes.map(|bytes| bytes as f64 / MIB), 0),
        number(
            cgroup
                .and_then(|usage| usage.cpu_throttled_fraction)
                .map(|f| f * 100.0),
            0
        ),
        number(cgroup.and_then(|usage| usage.cpu_pressure_avg10), 1),
        top_syscalls(row),
    )
}

fn top_syscalls(row: &Row) -> String {
    row.syscalls
        .iter()
        .take(TOP_SYSCALLS)
        .map(|rate| format!("{}:{:.0}", rate.name, rate.rate_per_sec))
        .collect::<Vec<_>>()
        .join(" ")
}

fn number(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{v:.decimals$}"))
}

fn truncate(text: &str, width: usize) -> &str {
    text.char_indices()
        .nth(width)
        .map_or(text, |(end, _)| &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ProcessUsage;

    fn row(unix_ms: u64, tgid: u32, cpu_cores: f64) -> Row {
        Row {
            unix_ms,
            tgid,
            process: Default::default(),
            workload: "no-gpu".into(),
            usage: ProcessUsage {
                cpu_cores: Some(cpu_cores),
                ..ProcessUsage::default()
            },
            cgroup_usage: None,
            syscalls: Vec::new(),
            label: None,
        }
    }

    #[test]
    fn newest_poll_drops_older_polls_and_sorts_by_cpu() {
        let rows = vec![row(1_000, 1, 3.0), row(3_000, 2, 0.5), row(3_020, 3, 1.5)];
        let tgids: Vec<u32> = newest_poll(rows).iter().map(|row| row.tgid).collect();
        assert_eq!(tgids, [3, 2]);
    }

    #[test]
    fn missing_values_render_as_dashes() {
        assert_eq!(number(None, 2), "-");
        assert_eq!(number(Some(1.234), 2), "1.23");
        assert_eq!(truncate("0123456789abcdef", SHORT_ID_LEN), "0123456789ab");
        assert_eq!(truncate("short", SHORT_ID_LEN), "short");
    }
}
