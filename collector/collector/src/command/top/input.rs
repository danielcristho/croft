//! Keys of croft top, read from the terminal without line buffering.

use std::{io::IsTerminal, thread, time::Duration};

use crate::runtime::signals;

const READ_BYTES: usize = 64;
const SEQUENCES: [(&[u8], Key); 21] = [
    (b"\x1b[A", Key::Up),
    (b"\x1bOA", Key::Up),
    (b"\x1b[B", Key::Down),
    (b"\x1bOB", Key::Down),
    (b"\x1b[5~", Key::PageUp),
    (b"\x1b[6~", Key::PageDown),
    (b"\x1b[H", Key::Home),
    (b"\x1bOH", Key::Home),
    (b"\x1b[1~", Key::Home),
    (b"\x1b[F", Key::End),
    (b"\x1bOF", Key::End),
    (b"\x1b[4~", Key::End),
    (b"q", Key::Quit),
    (b"Q", Key::Quit),
    (b"k", Key::Up),
    (b"j", Key::Down),
    (b"b", Key::PageUp),
    (b" ", Key::PageDown),
    (b"f", Key::PageDown),
    (b"g", Key::Home),
    (b"G", Key::End),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

// parse skips bytes it does not know, so an unknown escape sequence does nothing.
fn parse(mut bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    while !bytes.is_empty() {
        match SEQUENCES.iter().find(|(seq, _)| bytes.starts_with(seq)) {
            Some((seq, key)) => {
                keys.push(*key);
                bytes = &bytes[seq.len()..];
            }
            None if bytes[0] == 0x1b => bytes = skip_escape(bytes),
            None => bytes = &bytes[1..],
        }
    }
    keys
}

// skip_escape drops a whole unknown sequence, so its letters do not read as keys.
fn skip_escape(bytes: &[u8]) -> &[u8] {
    if bytes.get(1) != Some(&b'[') {
        return &bytes[1..];
    }
    let end = bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte));
    end.map_or(&[], |end| &bytes[end + 3..])
}

// Scroll is the first process row on screen, kept inside the rows of the current poll.
#[derive(Default)]
pub struct Scroll {
    offset: usize,
}

impl Scroll {
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn fit(&mut self, rows: usize, visible: usize) {
        self.offset = self.offset.min(rows.saturating_sub(visible));
    }

    pub fn apply(&mut self, key: Key, rows: usize, visible: usize) {
        self.offset = match key {
            Key::Up => self.offset.saturating_sub(1),
            Key::Down => self.offset + 1,
            Key::PageUp => self.offset.saturating_sub(visible),
            Key::PageDown => self.offset + visible,
            Key::Home => 0,
            Key::End => rows,
            Key::Quit => self.offset,
        };
        self.fit(rows, visible);
    }
}

// Keyboard turns off line buffering and echo while top runs; without a terminal on stdin no keys arrive.
pub struct Keyboard {
    saved: Option<libc::termios>,
    open: bool,
}

impl Keyboard {
    pub fn enter() -> Self {
        let saved = std::io::stdin().is_terminal().then(raw_mode).flatten();
        Self {
            saved,
            open: saved.is_some(),
        }
    }

    // read waits up to timeout and returns the keys pressed.
    pub fn read(&mut self, timeout: Duration) -> Vec<Key> {
        if !self.open {
            thread::sleep(timeout);
            return Vec::new();
        }
        let mut stdin = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: stdin points to one valid pollfd for the length of the call.
        let ready = unsafe { libc::poll(&mut stdin, 1, timeout.as_millis() as libc::c_int) };
        if ready <= 0 {
            return Vec::new();
        }
        let mut bytes = [0u8; READ_BYTES];
        // SAFETY: the buffer is valid for READ_BYTES bytes.
        let count =
            unsafe { libc::read(libc::STDIN_FILENO, bytes.as_mut_ptr().cast(), READ_BYTES) };
        if count <= 0 {
            self.open = false;
            return Vec::new();
        }
        parse(&bytes[..count as usize])
    }
}

// Drop puts the terminal back, so the shell echoes again after top exits.
impl Drop for Keyboard {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            // SAFETY: saved came from tcgetattr on the same descriptor.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, saved) };
        }
    }
}

// raw_mode returns the previous settings. Ctrl-C stays a signal; Ctrl-Z is off, since a stopped top would leave the screen behind.
fn raw_mode() -> Option<libc::termios> {
    // SAFETY: termios is plain data, filled by tcgetattr before use.
    unsafe {
        let mut settings: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut settings) != 0 {
            return None;
        }
        signals::saved_terminal(settings);
        let saved = settings;
        settings.c_lflag &= !(libc::ICANON | libc::ECHO);
        settings.c_cc[libc::VSUSP] = 0;
        settings.c_cc[libc::VMIN] = 1;
        settings.c_cc[libc::VTIME] = 0;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &settings) != 0 {
            return None;
        }
        Some(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_letters_and_escape_sequences() {
        assert_eq!(
            parse(b"q\x1b[A\x1bOB\x1b[5~\x1b[6~gG"),
            [
                Key::Quit,
                Key::Up,
                Key::Down,
                Key::PageUp,
                Key::PageDown,
                Key::Home,
                Key::End
            ]
        );
    }

    #[test]
    fn parse_skips_unknown_sequences_whole() {
        assert_eq!(parse(b"\x1b[1;5Qx\x1b[A"), [Key::Up]);
        assert_eq!(parse(b"\x1b[1;5"), []);
        assert_eq!(parse(b"\x1b"), []);
    }

    #[test]
    fn scroll_stays_inside_the_rows() {
        let mut scroll = Scroll::default();
        scroll.apply(Key::Up, 10, 4);
        assert_eq!(scroll.offset(), 0);
        scroll.apply(Key::PageDown, 10, 4);
        assert_eq!(scroll.offset(), 4);
        scroll.apply(Key::End, 10, 4);
        assert_eq!(scroll.offset(), 6);
        scroll.apply(Key::Down, 10, 4);
        assert_eq!(scroll.offset(), 6);
        scroll.fit(5, 4);
        assert_eq!(scroll.offset(), 1);
        scroll.apply(Key::Home, 5, 4);
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn scroll_stays_at_zero_when_all_rows_fit() {
        let mut scroll = Scroll::default();
        scroll.apply(Key::End, 3, 10);
        assert_eq!(scroll.offset(), 0);
    }
}
