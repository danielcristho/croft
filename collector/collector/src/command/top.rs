//! Live table of the processes in a dataset's newest poll.

mod view;

use std::{
    fs::File,
    io::{IsTerminal, Read, Seek, SeekFrom, Write},
    path::Path,
    thread,
    time::Duration,
};

use anyhow::Context;

use self::view::Style;
use crate::{
    ansi::{CLEAR_BELOW, CLEAR_LINE_END, ENTER_SCREEN, HOME, LEAVE_SCREEN},
    cli::TopArgs,
    dataset::now_unix_ms,
    rows::{self, Line, Row},
    signals,
};

// Consts for the number of bytes to read from the end of the dataset file, the time window for a poll, and the number of top syscalls to display.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;
const POLL_SPREAD_MS: u64 = 1_000;
const NO_COLOR_ENV: &str = "NO_COLOR";

// execute runs the top command, printing the newest poll of the dataset every interval seconds until interrupted.
pub fn execute(args: &TopArgs) -> anyhow::Result<()> {
    let path = &args.dataset.dataset;
    let style =
        Style::new(std::io::stdout().is_terminal() && std::env::var_os(NO_COLOR_ENV).is_none());
    let cores = thread::available_parallelism().map_or(1, |cores| cores.get());
    let frame = || -> anyhow::Result<String> {
        let poll = newest_poll(read_tail(path)?);
        Ok(view::render(
            &style,
            path,
            &poll,
            now_unix_ms() as u64,
            cores,
        ))
    };
    if args.once {
        print!("{}", frame()?);
        return Ok(());
    }
    let screen = Screen::enter();
    let mut stdout = std::io::stdout();
    loop {
        let frame = frame()?;
        if screen.active {
            write!(stdout, "{}", redraw(&frame))?;
        } else {
            writeln!(stdout, "{frame}")?;
        }
        stdout.flush()?;
        thread::sleep(Duration::from_secs(args.interval));
    }
}

// redraw overwrites the previous frame in place instead of clearing, so the view does not flicker.
fn redraw(frame: &str) -> String {
    let lines: String = frame
        .lines()
        .map(|line| format!("{line}{CLEAR_LINE_END}\n"))
        .collect();
    format!("{HOME}{lines}{CLEAR_BELOW}")
}

// Screen holds the alternate screen while top runs on a terminal; piped output gets plain frames.
struct Screen {
    active: bool,
}

impl Screen {
    fn enter() -> Self {
        let active = std::io::stdout().is_terminal();
        if active {
            print!("{ENTER_SCREEN}");
            signals::alternate_screen(true);
        }
        Self { active }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if self.active {
            print!("{LEAVE_SCREEN}");
            let _ = std::io::stdout().flush();
            signals::alternate_screen(false);
        }
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
    fn redraw_overwrites_each_line_and_clears_below() {
        let drawn = redraw("a\nbb\n");
        assert_eq!(
            drawn,
            format!("{HOME}a{CLEAR_LINE_END}\nbb{CLEAR_LINE_END}\n{CLEAR_BELOW}")
        );
    }
}
