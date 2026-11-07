//! Live table of the processes in a dataset's newest poll.

mod input;
mod view;

use std::{
    fs::File,
    io::{IsTerminal, Read, Seek, SeekFrom, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;

use self::{
    input::{Key, Keyboard, Scroll},
    view::Style,
};
use crate::{
    cli::TopArgs,
    pipeline::{
        dataset::now_unix_ms,
        rows::{self, Line, Row},
    },
    runtime::{
        ansi::{CLEAR_BELOW, CLEAR_LINE_END, ENTER_SCREEN, HOME, LEAVE_SCREEN},
        signals,
    },
};

// Consts for the number of bytes to read from the end of the dataset file, the time window for a poll, and the number of top syscalls to display.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;
const POLL_SPREAD_MS: u64 = 1_000;
const NO_COLOR_ENV: &str = "NO_COLOR";
const WAKE: Duration = Duration::from_millis(200);
const DEFAULT_SIZE: (usize, usize) = (24, 80);

// execute runs the top command: a scrolling view on a terminal, plain frames every interval seconds otherwise.
pub fn execute(args: &TopArgs) -> anyhow::Result<()> {
    let path = &args.dataset.dataset;
    let style =
        Style::new(std::io::stdout().is_terminal() && std::env::var_os(NO_COLOR_ENV).is_none());
    let cores = thread::available_parallelism().map_or(1, |cores| cores.get());
    if args.once {
        let poll = newest_poll(read_tail(path)?);
        print!(
            "{}",
            view::render(&style, path, &poll, now_unix_ms() as u64, cores)
        );
        return Ok(());
    }
    if std::io::stdout().is_terminal() {
        return interactive(args, &style, cores);
    }
    let mut stdout = std::io::stdout();
    loop {
        let poll = newest_poll(read_tail(path)?);
        let frame = view::render(&style, path, &poll, now_unix_ms() as u64, cores);
        writeln!(stdout, "{frame}")?;
        stdout.flush()?;
        thread::sleep(Duration::from_secs(args.interval));
    }
}

// interactive reads the dataset every interval and redraws on new rows, a key or a resize, until q.
fn interactive(args: &TopArgs, style: &Style, cores: usize) -> anyhow::Result<()> {
    let path = &args.dataset.dataset;
    let interval = Duration::from_secs(args.interval);
    let _screen = Screen::enter();
    let mut keyboard = Keyboard::enter();
    let mut stdout = std::io::stdout();
    let mut scroll = Scroll::default();
    let mut poll = newest_poll(read_tail(path)?);
    let mut next_read = Instant::now() + interval;
    let mut size = (0, 0);
    let mut dirty = true;
    loop {
        if Instant::now() >= next_read {
            poll = newest_poll(read_tail(path)?);
            next_read = Instant::now() + interval;
            dirty = true;
        }
        let current = terminal_size();
        if current != size {
            size = current;
            dirty = true;
        }
        let (height, width) = size;
        let visible = view::visible_rows(height);
        if dirty {
            scroll.fit(poll.len(), visible);
            let frame = view::window(
                style,
                path,
                &poll,
                now_unix_ms() as u64,
                cores,
                scroll.offset(),
                visible,
            );
            write!(stdout, "{}", redraw(&frame, height, width))?;
            stdout.flush()?;
            dirty = false;
        }
        let wait = WAKE.min(next_read.saturating_duration_since(Instant::now()));
        for key in keyboard.read(wait) {
            if key == Key::Quit {
                return Ok(());
            }
            scroll.apply(key, poll.len(), visible);
            dirty = true;
        }
    }
}

// terminal_size asks the terminal for its rows and columns, since a resize can come at any time.
fn terminal_size() -> (usize, usize) {
    // SAFETY: winsize is plain data, and TIOCGWINSZ only writes to it.
    let size = unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        match libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) {
            0 => (size.ws_row as usize, size.ws_col as usize),
            _ => (0, 0),
        }
    };
    if size.0 == 0 || size.1 == 0 {
        DEFAULT_SIZE
    } else {
        size
    }
}

// redraw overwrites the previous frame in place instead of clearing, so the view does not flicker; no newline follows the last line, which would scroll the screen.
fn redraw(frame: &str, height: usize, width: usize) -> String {
    let lines: Vec<String> = frame
        .lines()
        .take(height)
        .map(|line| format!("{}{CLEAR_LINE_END}", clip(line, width)))
        .collect();
    format!("{HOME}{}{CLEAR_BELOW}", lines.join("\n"))
}

// clip drops the characters past width; colour codes take no width and all stay, so each reset still arrives.
fn clip(line: &str, width: usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut shown = 0;
    let mut escape = false;
    for c in line.chars() {
        if escape || c == '\x1b' {
            out.push(c);
            escape = c != 'm';
        } else if shown < width {
            out.push(c);
            shown += 1;
        }
    }
    out
}

// Screen holds the alternate screen while top runs on a terminal; piped output gets plain frames.
struct Screen;

impl Screen {
    fn enter() -> Self {
        print!("{ENTER_SCREEN}");
        signals::alternate_screen(true);
        Self
    }
}

// Drop leaves the alternate screen when top exits, so the terminal is restored to its previous state.
impl Drop for Screen {
    fn drop(&mut self) {
        print!("{LEAVE_SCREEN}");
        let _ = std::io::stdout().flush();
        signals::alternate_screen(false);
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
    use crate::inspect::resources::ProcessUsage;

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
    fn clip_counts_visible_characters_and_keeps_colour_codes() {
        assert_eq!(clip("\x1b[31mabcd\x1b[0mef", 3), "\x1b[31mabc\x1b[0m");
        assert_eq!(clip("ab", 3), "ab");
    }

    #[test]
    fn redraw_overwrites_each_line_and_ends_without_a_newline() {
        let drawn = redraw("a\nbb\nc\n", 2, 80);
        assert_eq!(
            drawn,
            format!("{HOME}a{CLEAR_LINE_END}\nbb{CLEAR_LINE_END}{CLEAR_BELOW}")
        );
    }
}
