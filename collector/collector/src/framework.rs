//! Detects the ML framework a process runs from the libraries mapped into it.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    Pytorch,
    Tensorflow,
    Jax,
}

impl Framework {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pytorch => "pytorch",
            Self::Tensorflow => "tensorflow",
            Self::Jax => "jax",
        }
    }
}

// Library file name prefixes per framework, checked in this order when a process maps more than one.
const LIBRARIES: &[(Framework, &[&str])] = &[
    (Framework::Pytorch, &["libtorch_cpu.so", "libtorch_cuda.so"]),
    (
        Framework::Tensorflow,
        &[
            "libtensorflow_framework.so",
            "libtensorflow_cc.so",
            "_pywrap_tensorflow_internal.so",
        ],
    ),
    (Framework::Jax, &["xla_extension.so"]),
];
// jaxlib renames its extension between releases; its directory stays.
const JAX_DIR: &str = "/jaxlib/";

// detect reads the lines of a /proc/<pid>/maps listing; with several frameworks mapped, the first in LIBRARIES wins.
pub fn detect<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> Option<Framework> {
    let found: HashSet<Framework> = lines
        .into_iter()
        .filter_map(|line| path_of(line.as_ref()).and_then(framework_of))
        .collect();
    LIBRARIES
        .iter()
        .map(|(framework, _)| *framework)
        .find(|framework| found.contains(framework))
}

// path_of takes the path column; anonymous mappings have none and special ones such as [stack] are not files.
fn path_of(line: &str) -> Option<&str> {
    line.split_whitespace()
        .nth(5)
        .filter(|path| path.starts_with('/'))
}

fn framework_of(path: &str) -> Option<Framework> {
    let name = path.rsplit('/').next()?;
    LIBRARIES
        .iter()
        .find(|(_, prefixes)| prefixes.iter().any(|prefix| name.starts_with(prefix)))
        .map(|(framework, _)| *framework)
        .or_else(|| path.contains(JAX_DIR).then_some(Framework::Jax))
}

fn read_maps(tgid: u32) -> Option<Framework> {
    let file = File::open(format!("/proc/{tgid}/maps")).ok()?;
    detect(BufReader::new(file).lines().map_while(Result::ok))
}

#[derive(Clone, Copy)]
struct Seen {
    first_poll: u64,
    framework: Option<Framework>,
}

// FrameworkTracker reads a process's maps until a framework shows up or the process has been seen for `recheck_polls`.
pub struct FrameworkTracker {
    recheck_polls: u64,
    poll: u64,
    known: HashMap<u32, Seen>,
    seen: HashSet<u32>,
}

impl FrameworkTracker {
    pub fn new(recheck_polls: u64) -> Self {
        Self {
            recheck_polls,
            poll: 0,
            known: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    pub fn framework(&mut self, tgid: u32) -> Option<Framework> {
        self.lookup(tgid, read_maps)
    }

    // lookup keeps a found framework; a process with none is read again while it is younger than recheck_polls.
    fn lookup(
        &mut self,
        tgid: u32,
        read: impl FnOnce(u32) -> Option<Framework>,
    ) -> Option<Framework> {
        self.seen.insert(tgid);
        let poll = self.poll;
        let entry = self.known.entry(tgid).or_insert(Seen {
            first_poll: poll,
            framework: None,
        });
        if entry.framework.is_none() && poll - entry.first_poll < self.recheck_polls {
            entry.framework = read(tgid);
        }
        entry.framework
    }

    // end_poll forgets processes not seen in this poll, so a reused tgid starts over.
    pub fn end_poll(&mut self) {
        self.poll += 1;
        let seen = std::mem::take(&mut self.seen);
        self.known.retain(|tgid, _| seen.contains(tgid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TORCH: &str = "\
7f1c2a000000-7f1c2a400000 r-xp 00000000 08:01 1234 /usr/local/lib/python3.11/site-packages/torch/lib/libtorch_cpu.so
7f1c2b000000-7f1c2b100000 r-xp 00000000 08:01 1235 /usr/local/lib/python3.11/site-packages/torch/lib/libtorch_cuda.so
7ffd1c000000-7ffd1c021000 rw-p 00000000 00:00 0 [stack]";
    const TENSORFLOW: &str = "\
7f00aa000000-7f00ab000000 r-xp 00000000 08:01 42 /usr/local/lib/python3.11/dist-packages/tensorflow/libtensorflow_framework.so.2
7f00ac000000-7f00ad000000 r-xp 00000000 08:01 43 /usr/local/lib/python3.11/dist-packages/tensorflow/python/_pywrap_tensorflow_internal.so";
    const JAX: &str = "\
7f00aa000000-7f00ab000000 r-xp 00000000 08:01 42 /opt/venv/lib/python3.12/site-packages/jaxlib/_jax.so";
    const PLAIN: &str = "\
55d0c1a00000-55d0c1b00000 r-xp 00000000 08:01 7 /usr/bin/python3.12
7f00aa000000-7f00ab000000 r-xp 00000000 08:01 8 /usr/lib/x86_64-linux-gnu/libc.so.6
7f00ac000000-7f00ac001000 rw-p 00000000 00:00 0
7ffd1c000000-7ffd1c021000 rw-p 00000000 00:00 0 [vdso]";

    fn from(maps: &str) -> Option<Framework> {
        detect(maps.lines())
    }

    #[test]
    fn detects_each_framework() {
        assert_eq!(from(TORCH), Some(Framework::Pytorch));
        assert_eq!(from(TENSORFLOW), Some(Framework::Tensorflow));
        assert_eq!(from(JAX), Some(Framework::Jax));
        assert_eq!(from(PLAIN), None);
    }

    #[test]
    fn pytorch_wins_over_tensorflow_in_one_process() {
        assert_eq!(
            from(&format!("{TENSORFLOW}\n{TORCH}")),
            Some(Framework::Pytorch)
        );
        assert_eq!(
            from(&format!("{JAX}\n{TENSORFLOW}")),
            Some(Framework::Tensorflow)
        );
    }

    #[test]
    fn library_names_match_on_the_file_not_the_directory() {
        let maps = "7f-8f r-xp 0 08:01 1 /home/libtorch_cpu.so.d/notes.txt";
        assert_eq!(from(maps), None);
    }

    #[test]
    fn label_matches_the_dataset_value() {
        for framework in [Framework::Pytorch, Framework::Tensorflow, Framework::Jax] {
            let json = serde_json::to_string(&framework).unwrap();
            assert_eq!(json, format!("\"{}\"", framework.label()));
        }
    }

    #[test]
    fn rereads_until_found_then_keeps_it() {
        let mut tracker = FrameworkTracker::new(5);
        let mut reads = 0;
        let mut poll = |tracker: &mut FrameworkTracker, answer| {
            let got = tracker.lookup(7, |_| {
                reads += 1;
                answer
            });
            tracker.end_poll();
            got
        };
        assert_eq!(poll(&mut tracker, None), None);
        assert_eq!(
            poll(&mut tracker, Some(Framework::Pytorch)),
            Some(Framework::Pytorch)
        );
        assert_eq!(poll(&mut tracker, None), Some(Framework::Pytorch));
        assert_eq!(reads, 2);
    }

    #[test]
    fn stops_rereading_after_recheck_polls() {
        let mut tracker = FrameworkTracker::new(2);
        let mut reads = 0;
        for _ in 0..4 {
            tracker.lookup(7, |_| {
                reads += 1;
                None
            });
            tracker.end_poll();
        }
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_process_gone_for_a_poll_starts_over() {
        let mut tracker = FrameworkTracker::new(1);
        tracker.lookup(7, |_| None);
        tracker.end_poll();
        tracker.end_poll();
        assert_eq!(
            tracker.lookup(7, |_| Some(Framework::Jax)),
            Some(Framework::Jax)
        );
    }
}
