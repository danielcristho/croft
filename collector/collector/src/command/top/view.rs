//! Frame of croft top: title bar, meters, and the process table, coloured like htop when drawn on a terminal.

use std::path::Path;

use crate::{
    pipeline::rows::Row,
    runtime::ansi::{
        BAR_HEADER, BAR_TITLE, BOLD, BOLD_BLUE, BOLD_GREEN, BOLD_MAGENTA, BOLD_YELLOW, CYAN,
        DIM_GREY, GREEN, GREY, RED, RESET, YELLOW,
    },
};

// Constants for the top command's layout and thresholds.
const TOP_SYSCALLS: usize = 3;
const SHORT_ID_LEN: usize = 12;
const COMM_LEN: usize = 15;
const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MIB;
const METER_CELLS: usize = 40;
const CPU_CELLS: usize = 10;
const CPU_HIGH: f64 = 0.9;
const CPU_MID: f64 = 0.5;
const CPU_IDLE: f64 = 0.05;
const STALE_MS: u64 = 4_000;
const THROTTLED_HIGH: f64 = 20.0;
const THROTTLED_MID: f64 = 5.0;
const PSI_HIGH: f64 = 10.0;
const PSI_MID: f64 = 2.0;

pub struct Style {
    color: bool,
}

impl Style {
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }
}

enum Align {
    Left,
    Right,
}

struct Column {
    title: &'static str,
    width: usize,
    align: Align,
}

const COLUMNS: [Column; 8] = [
    Column {
        title: "TGID",
        width: 8,
        align: Align::Right,
    },
    Column {
        title: "COMM",
        width: COMM_LEN,
        align: Align::Left,
    },
    Column {
        title: "CONTAINER",
        width: SHORT_ID_LEN,
        align: Align::Left,
    },
    Column {
        title: "WORKLOAD",
        width: 9,
        align: Align::Left,
    },
    Column {
        title: "CPU",
        width: 5 + 1 + CPU_CELLS,
        align: Align::Left,
    },
    Column {
        title: "RSS_MIB",
        width: 7,
        align: Align::Right,
    },
    Column {
        title: "THROT%",
        width: 6,
        align: Align::Right,
    },
    Column {
        title: "PSI%",
        width: 5,
        align: Align::Right,
    },
];

// render builds the frame of croft top, with a title bar, meters, and a table of processes.
pub fn render(style: &Style, path: &Path, poll: &[Row], now_ms: u64, cores: usize) -> String {
    let mut out = title(style, path, poll, now_ms);
    out.push_str(&meters(style, poll, cores));
    out.push('\n');
    out.push_str(&header(style));
    for row in poll {
        out.push_str(&line(style, row));
        out.push('\n');
    }
    out
}

// title shows the path, number of processes, and age of the newest row, in red if stale.
fn title(style: &Style, path: &Path, poll: &[Row], now_ms: u64) -> String {
    let age = match poll.first() {
        None => style.paint(RED, "no rows"),
        Some(row) => {
            let age_ms = now_ms.saturating_sub(row.unix_ms);
            let text = format!("newest row {}s ago", age_ms / 1000);
            style.paint(if age_ms > STALE_MS { RED } else { GREEN }, &text)
        }
    };
    format!(
        "{} {}  {} processes  {age}\n",
        style.paint(BAR_TITLE, " croft top "),
        style.paint(GREY, &path.display().to_string()),
        style.paint(BOLD, &poll.len().to_string()),
    )
}

// meters sums the poll like htop's header: CPU against the host's cores, memory, and processes per workload.
fn meters(style: &Style, poll: &[Row], cores: usize) -> String {
    let cpu: f64 = poll
        .iter()
        .map(|row| row.usage.cpu_cores.unwrap_or(0.0))
        .sum();
    let rss: f64 = poll
        .iter()
        .map(|row| row.usage.rss_bytes.unwrap_or(0) as f64)
        .sum();
    let count = |workload: &str| poll.iter().filter(|row| row.workload == workload).count();
    format!(
        "{}{}{} {} / {cores} cores   {} {:.1} GiB   {} {}  {} {}  {} {}\n",
        style.paint(BOLD, "CPU ["),
        bar(style, cpu / cores.max(1) as f64, METER_CELLS),
        style.paint(BOLD, "]"),
        style.paint(BOLD, &format!("{cpu:.2}")),
        style.paint(BOLD, "RSS"),
        rss / GIB,
        style.paint(BOLD_GREEN, "gpu-onset"),
        count("gpu-onset"),
        style.paint(BOLD_YELLOW, "gpu-held"),
        count("gpu-held"),
        style.paint(BOLD_BLUE, "no-gpu"),
        count("no-gpu"),
    )
}

// bar fills cells by fraction; each cell takes the colour of its position, green to red, as btop draws its meters.
fn bar(style: &Style, fraction: f64, cells: usize) -> String {
    let filled = ((fraction.clamp(0.0, 1.0) * cells as f64).round() as usize).min(cells);
    let mut out = String::new();
    for cell in 0..cells {
        if cell >= filled {
            out.push_str(&style.paint(DIM_GREY, "·"));
            continue;
        }
        let position = cell as f64 / cells as f64;
        let code = if position >= 0.8 {
            RED
        } else if position >= 0.5 {
            YELLOW
        } else {
            GREEN
        };
        out.push_str(&style.paint(code, "|"));
    }
    out
}

// header shows the column titles, with the rightmost column showing the top syscalls per second.
fn header(style: &Style) -> String {
    let cells: Vec<String> = COLUMNS
        .iter()
        .map(|column| pad(column.title, column))
        .collect();
    let text = format!("{}  {:<24}", cells.join(" "), "TOP SYSCALLS/S");
    format!("{}\n", style.paint(BAR_HEADER, &text))
}

// line shows a row of the process table, with the rightmost column showing the top syscalls per second.
fn line(style: &Style, row: &Row) -> String {
    let cgroup = row.cgroup_usage.as_ref();
    let cpu = row.usage.cpu_cores;
    let throttled = cgroup
        .and_then(|usage| usage.cpu_throttled_fraction)
        .map(|fraction| fraction * 100.0);
    let pressure = cgroup.and_then(|usage| usage.cpu_pressure_avg10);
    let cells = [
        style.paint(BOLD_MAGENTA, &pad(&row.tgid.to_string(), &COLUMNS[0])),
        style.paint(
            BOLD,
            &pad(row.process.comm.as_deref().unwrap_or("?"), &COLUMNS[1]),
        ),
        style.paint(
            GREY,
            &pad(
                row.process.container_id.as_deref().unwrap_or("-"),
                &COLUMNS[2],
            ),
        ),
        style.paint(
            workload_color(&row.workload),
            &pad(&row.workload, &COLUMNS[3]),
        ),
        format!(
            "{} {}",
            style.paint(
                level(cpu, CPU_HIGH, CPU_MID, CPU_IDLE),
                &format!("{:>5}", number(cpu, 2))
            ),
            bar(style, cpu.unwrap_or(0.0), CPU_CELLS)
        ),
        pad(
            &number(row.usage.rss_bytes.map(|bytes| bytes as f64 / MIB), 0),
            &COLUMNS[5],
        ),
        style.paint(
            level(throttled, THROTTLED_HIGH, THROTTLED_MID, 0.0),
            &pad(&number(throttled, 0), &COLUMNS[6]),
        ),
        style.paint(
            level(pressure, PSI_HIGH, PSI_MID, 0.0),
            &pad(&number(pressure, 1), &COLUMNS[7]),
        ),
    ];
    format!("{}  {}", cells.join(" "), top_syscalls(style, row))
}

// workload_color picks a colour for the workload column, to match the meters.
fn workload_color(workload: &str) -> &'static str {
    match workload {
        "gpu-onset" => BOLD_GREEN,
        "gpu-held" => BOLD_YELLOW,
        "no-gpu" => BOLD_BLUE,
        _ => BOLD,
    }
}

// level picks red at or above high, yellow at or above mid, green above idle, and grey for idle or missing values.
fn level(value: Option<f64>, high: f64, mid: f64, idle: f64) -> &'static str {
    match value {
        Some(v) if v >= high => RED,
        Some(v) if v >= mid => YELLOW,
        Some(v) if v > idle => GREEN,
        _ => DIM_GREY,
    }
}

// top_syscalls shows the top syscalls per second, up to a limit, with the syscall name in cyan and the rate in plain text.
fn top_syscalls(style: &Style, row: &Row) -> String {
    row.syscalls
        .iter()
        .take(TOP_SYSCALLS)
        .map(|rate| {
            format!(
                "{}{}",
                style.paint(CYAN, &format!("{}:", rate.name)),
                format_args!("{:.0}", rate.rate_per_sec)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// pad truncates and aligns plain text before it is painted, so colour codes never count toward the width.
fn pad(text: &str, column: &Column) -> String {
    let text = truncate(text, column.width);
    match column.align {
        Align::Left => format!("{text:<width$}", width = column.width),
        Align::Right => format!("{text:>width$}", width = column.width),
    }
}

// number formats a floating point value to a string with the given number of decimal places, or "-" if missing.
fn number(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{v:.decimals$}"))
}

// truncate returns a slice of the text up to the given width in characters, or the whole text if shorter.
fn truncate(text: &str, width: usize) -> &str {
    text.char_indices()
        .nth(width)
        .map_or(text, |(end, _)| &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::resources::ProcessUsage;

    fn row(workload: &str, cpu_cores: f64) -> Row {
        Row {
            unix_ms: 10_000,
            tgid: 7,
            process: Default::default(),
            workload: workload.into(),
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
    fn plain_frames_carry_no_escape_codes() {
        let frame = render(
            &Style::new(false),
            Path::new("d.jsonl"),
            &[row("gpu-held", 0.5)],
            10_500,
            4,
        );
        assert!(!frame.contains('\x1b'));
        assert!(frame.contains("gpu-held"));
    }

    #[test]
    fn coloured_rows_keep_the_plain_column_layout() {
        let rows = [row("no-gpu", 0.95)];
        let plain = render(&Style::new(false), Path::new("d"), &rows, 10_000, 4);
        let coloured = render(&Style::new(true), Path::new("d"), &rows, 10_000, 4);
        let strip = |text: &str| {
            let mut out = String::new();
            let mut escape = false;
            for c in text.chars() {
                match (escape, c) {
                    (false, '\x1b') => escape = true,
                    (true, 'm') => escape = false,
                    (false, _) => out.push(c),
                    _ => {}
                }
            }
            out
        };
        assert_eq!(strip(&coloured), plain);
        assert!(coloured.contains(&format!("{BOLD_BLUE}no-gpu")));
    }

    #[test]
    fn levels_follow_the_thresholds() {
        for (value, code) in [
            (Some(0.95), RED),
            (Some(0.6), YELLOW),
            (Some(0.2), GREEN),
            (Some(0.0), DIM_GREY),
            (None, DIM_GREY),
        ] {
            assert_eq!(level(value, CPU_HIGH, CPU_MID, CPU_IDLE), code, "{value:?}");
        }
    }

    #[test]
    fn bars_fill_by_fraction() {
        let plain = Style::new(false);
        assert_eq!(bar(&plain, 0.5, 4), "||··");
        assert_eq!(bar(&plain, 2.0, 4), "||||");
        assert_eq!(bar(&plain, -1.0, 4), "····");
    }

    #[test]
    fn missing_values_render_as_dashes() {
        assert_eq!(number(None, 2), "-");
        assert_eq!(number(Some(1.234), 2), "1.23");
        assert_eq!(truncate("0123456789abcdef", SHORT_ID_LEN), "0123456789ab");
        assert_eq!(truncate("short", SHORT_ID_LEN), "short");
    }
}
