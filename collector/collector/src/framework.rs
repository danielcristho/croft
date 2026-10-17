//! Framework detection reads a process's /proc/<pid>/maps to find a framework in the catalog.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
};

use anyhow::{Context, bail};
use serde::Deserialize;

// Constant catalog is built into the binary, but can be overridden by a file path in the environment.
const DEFAULT_CATALOG: &str = include_str!("../frameworks.json");
pub const ENV: &str = "CROFT_FRAMEWORKS";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    #[serde(default)]
    libraries: Vec<String>,
    #[serde(default)]
    directories: Vec<String>,
}

#[derive(Debug)]
pub struct Catalog {
    entries: Vec<Entry>,
}

impl Catalog {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        match path {
            Some(path) => {
                let text =
                    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
                Self::parse(&text).with_context(|| format!("framework catalog {}", path.display()))
            }
            None => Self::parse(DEFAULT_CATALOG).context("built-in framework catalog"),
        }
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let entries: Vec<Entry> = serde_json::from_str(text)?;
        let mut names = HashSet::new();
        for entry in &entries {
            if entry.name.is_empty() {
                bail!("an entry has an empty name");
            }
            if !names.insert(entry.name.as_str()) {
                bail!("{} is listed twice", entry.name);
            }
            if entry.libraries.is_empty() && entry.directories.is_empty() {
                bail!("{} has no libraries or directories to match", entry.name);
            }
        }
        Ok(Self { entries })
    }

    pub fn name(&self, index: usize) -> &str {
        &self.entries[index].name
    }

    // detect reads the lines of a /proc/<pid>/maps listing and returns the index of the earliest entry that matches.
    pub fn detect<S: AsRef<str>>(&self, lines: impl IntoIterator<Item = S>) -> Option<usize> {
        let mut best: Option<usize> = None;
        for line in lines {
            let Some(index) = path_of(line.as_ref()).and_then(|path| self.entry_of(path)) else {
                continue;
            };
            if index == 0 {
                return Some(0);
            }
            best = Some(best.map_or(index, |best| best.min(index)));
        }
        best
    }

    // entry_of returns the index of the first entry that matches the path, or None if none match.
    fn entry_of(&self, path: &str) -> Option<usize> {
        let name = path.rsplit('/').next()?;
        self.entries.iter().position(|entry| {
            entry
                .libraries
                .iter()
                .any(|prefix| name.starts_with(prefix.as_str()))
                || entry
                    .directories
                    .iter()
                    .any(|dir| path.contains(dir.as_str()))
        })
    }

    // read_maps reads a process's /proc/<pid>/maps and returns the index of the earliest entry that matches, or None if none match.
    fn read_maps(&self, tgid: u32) -> Option<usize> {
        let file = File::open(format!("/proc/{tgid}/maps")).ok()?;
        self.detect(BufReader::new(file).lines().map_while(Result::ok))
    }
}

// path_of takes the path column; anonymous mappings have none and special ones such as [stack] are not files.
fn path_of(line: &str) -> Option<&str> {
    line.split_whitespace()
        .nth(5)
        .filter(|path| path.starts_with('/'))
}

#[derive(Clone, Copy)]
struct Seen {
    first_poll: u64,
    framework: Option<usize>,
}

// FrameworkTracker reads a process's maps until a framework shows up or the process has been seen for `recheck_polls`.
pub struct FrameworkTracker {
    catalog: Catalog,
    recheck_polls: u64,
    poll: u64,
    known: HashMap<u32, Seen>,
    seen: HashSet<u32>,
}

impl FrameworkTracker {
    pub fn new(catalog: Catalog, recheck_polls: u64) -> Self {
        Self {
            catalog,
            recheck_polls,
            poll: 0,
            known: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    pub fn framework(&mut self, tgid: u32) -> Option<String> {
        let index = self.lookup(tgid, |catalog, tgid| catalog.read_maps(tgid))?;
        Some(self.catalog.name(index).to_string())
    }

    // lookup returns the index of the framework for a tgid, reading it if not known and not too many polls have passed.
    fn lookup(
        &mut self,
        tgid: u32,
        read: impl FnOnce(&Catalog, u32) -> Option<usize>,
    ) -> Option<usize> {
        self.seen.insert(tgid);
        let poll = self.poll;
        let entry = self.known.entry(tgid).or_insert(Seen {
            first_poll: poll,
            framework: None,
        });
        if entry.framework.is_none() && poll - entry.first_poll < self.recheck_polls {
            entry.framework = read(&self.catalog, tgid);
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

    fn from(maps: &str) -> Option<String> {
        let catalog = Catalog::load(None).unwrap();
        catalog
            .detect(maps.lines())
            .map(|i| catalog.name(i).to_string())
    }

    #[test]
    fn built_in_catalog_detects_each_framework() {
        assert_eq!(from(TORCH).as_deref(), Some("pytorch"));
        assert_eq!(from(TENSORFLOW).as_deref(), Some("tensorflow"));
        assert_eq!(from(JAX).as_deref(), Some("jax"));
        assert_eq!(from(PLAIN), None);
    }

    #[test]
    fn earlier_entry_wins_in_one_process() {
        assert_eq!(
            from(&format!("{TENSORFLOW}\n{TORCH}")).as_deref(),
            Some("pytorch")
        );
        assert_eq!(
            from(&format!("{JAX}\n{TENSORFLOW}")).as_deref(),
            Some("tensorflow")
        );
    }

    #[test]
    fn library_names_match_on_the_file_not_the_directory() {
        assert_eq!(
            from("7f-8f r-xp 0 08:01 1 /home/libtorch_cpu.so.d/notes.txt"),
            None
        );
    }

    #[test]
    fn a_catalog_file_replaces_the_built_in_one() {
        let path =
            std::env::temp_dir().join(format!("croft-frameworks-{}.json", std::process::id()));
        fs::write(
            &path,
            r#"[{"name": "onnxruntime", "libraries": ["libonnxruntime.so"]}]"#,
        )
        .unwrap();
        let catalog = Catalog::load(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();
        let maps = "7f-8f r-xp 0 08:01 1 /opt/lib/libonnxruntime.so.1.20";
        assert_eq!(
            catalog.detect(maps.lines()).map(|i| catalog.name(i)),
            Some("onnxruntime")
        );
        assert_eq!(catalog.detect(TORCH.lines()), None);
    }

    #[test]
    fn invalid_catalogs_are_refused() {
        assert!(Catalog::parse("not json").is_err());
        assert!(
            Catalog::parse(
                r#"[{"name": "a", "libraries": ["x"]}, {"name": "a", "libraries": ["y"]}]"#
            )
            .is_err()
        );
        assert!(Catalog::parse(r#"[{"name": "a"}]"#).is_err());
        assert!(Catalog::parse(r#"[{"name": "", "libraries": ["x"]}]"#).is_err());
        assert!(Catalog::parse(r#"[{"name": "a", "libs": ["x"]}]"#).is_err());
    }

    fn tracker(recheck_polls: u64) -> FrameworkTracker {
        FrameworkTracker::new(Catalog::load(None).unwrap(), recheck_polls)
    }

    #[test]
    fn rereads_until_found_then_keeps_it() {
        let mut tracker = tracker(5);
        let mut reads = 0;
        let mut poll = |tracker: &mut FrameworkTracker, answer| {
            let got = tracker.lookup(7, |_, _| {
                reads += 1;
                answer
            });
            tracker.end_poll();
            got
        };
        assert_eq!(poll(&mut tracker, None), None);
        assert_eq!(poll(&mut tracker, Some(0)), Some(0));
        assert_eq!(poll(&mut tracker, None), Some(0));
        assert_eq!(reads, 2);
    }

    #[test]
    fn stops_rereading_after_recheck_polls() {
        let mut tracker = tracker(2);
        let mut reads = 0;
        for _ in 0..4 {
            tracker.lookup(7, |_, _| {
                reads += 1;
                None
            });
            tracker.end_poll();
        }
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_process_gone_for_a_poll_starts_over() {
        let mut tracker = tracker(1);
        tracker.lookup(7, |_, _| None);
        tracker.end_poll();
        tracker.end_poll();
        assert_eq!(tracker.lookup(7, |_, _| Some(2)), Some(2));
    }
}
