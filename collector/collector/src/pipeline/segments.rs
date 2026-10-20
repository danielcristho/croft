//! Rotated dataset segments: the live file is renamed once it is old or large, compressed with zstd, and pruned.

use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

use log::{info, warn};

// Consts for the file suffixes, the zstd level, and the length of a segment's UTC stamp.
pub const LIVE_SUFFIX: &str = ".jsonl";
pub const COMPRESSED_SUFFIX: &str = ".jsonl.zst";
const PART_SUFFIX: &str = ".part";
// Level 3 shrinks rows about 13x at over 1 GB/s; higher levels gain little for several times the CPU.
const ZSTD_LEVEL: i32 = 3;
const STAMP_LEN: usize = "20261020T090000Z".len();
const SECS_PER_DAY: u64 = 86_400;

// Policy holds the rotation and retention limits; None turns a limit off.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub rotate_after: Option<Duration>,
    pub rotate_bytes: Option<u64>,
    pub retain: Option<Duration>,
    pub retain_bytes: Option<u64>,
}

impl Policy {
    // is_due tells whether a live file of this age and size should become a segment.
    pub fn is_due(&self, age: Duration, bytes: u64) -> bool {
        self.rotate_after.is_some_and(|after| age >= after)
            || self.rotate_bytes.is_some_and(|limit| bytes >= limit)
    }
}

// Segment is a rotated file, named <stem>-<UTC start>.jsonl and then .jsonl.zst.
#[derive(Debug, PartialEq)]
pub struct Segment {
    pub path: PathBuf,
    pub stamp: String,
    pub compressed: bool,
}

// segment_name names the segment that starts at start_ms.
pub fn segment_name(stem: &str, start_ms: u64) -> String {
    format!("{stem}-{}{LIVE_SUFFIX}", utc_stamp(start_ms))
}

// parse_segment reads the start stamp out of a segment's file name, or None for any other file.
pub fn parse_segment(stem: &str, file_name: &str) -> Option<(String, bool)> {
    let rest = file_name.strip_prefix(stem)?.strip_prefix('-')?;
    let (stamp, compressed) = match rest.strip_suffix(COMPRESSED_SUFFIX) {
        Some(stamp) => (stamp, true),
        None => (rest.strip_suffix(LIVE_SUFFIX)?, false),
    };
    is_stamp(stamp).then(|| (stamp.to_string(), compressed))
}

// is_segment_name tells whether a file name is a segment of any stem.
pub fn is_segment_name(file_name: &str) -> bool {
    let base = file_name
        .strip_suffix(COMPRESSED_SUFFIX)
        .or_else(|| file_name.strip_suffix(LIVE_SUFFIX));
    base.and_then(|base| base.len().checked_sub(STAMP_LEN + 1).map(|at| &base[at..]))
        .and_then(|tail| tail.strip_prefix('-'))
        .is_some_and(is_stamp)
}

fn is_stamp(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    bytes.len() == STAMP_LEN
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && bytes[..8]
            .iter()
            .chain(&bytes[9..15])
            .all(u8::is_ascii_digit)
}

// list returns the segments of stem in dir, oldest first.
pub fn list(dir: &Path, stem: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (stamp, compressed) = parse_segment(stem, &name)?;
            Some(Segment {
                path: entry.path(),
                stamp,
                compressed,
            })
        })
        .collect();
    segments.sort_by(|a, b| a.stamp.cmp(&b.stamp).then(a.compressed.cmp(&b.compressed)));
    segments
}

// rotate renames the live file to the segment that started at start_ms.
pub fn rotate(live: &Path, stem: &str, start_ms: u64) -> io::Result<PathBuf> {
    let segment = live.with_file_name(segment_name(stem, start_ms));
    fs::rename(live, &segment)?;
    Ok(segment)
}

// compress writes segment.zst beside the segment, then removes the segment; a crash leaves only a .part to discard.
pub fn compress(segment: &Path) -> io::Result<PathBuf> {
    let mut name = segment.as_os_str().to_owned();
    name.push(".zst");
    let target = PathBuf::from(name);
    let mut part = target.as_os_str().to_owned();
    part.push(PART_SUFFIX);
    let part = PathBuf::from(part);
    let output = File::create(&part)?;
    let mut encoder = zstd::Encoder::new(output, ZSTD_LEVEL)?;
    io::copy(&mut File::open(segment)?, &mut encoder)?;
    encoder.finish()?.sync_all()?;
    fs::rename(&part, &target)?;
    fs::remove_file(segment)?;
    Ok(target)
}

// expired picks the compressed segments to delete: older than retain by mtime, then oldest first past retain_bytes.
pub fn expired(
    segments: &[(PathBuf, SystemTime, u64)],
    policy: &Policy,
    now: SystemTime,
) -> Vec<PathBuf> {
    let mut kept_bytes: u64 = segments.iter().map(|(_, _, bytes)| bytes).sum();
    let mut removed = Vec::new();
    for (path, modified, bytes) in segments {
        let too_old = policy
            .retain
            .is_some_and(|retain| now.duration_since(*modified).unwrap_or_default() > retain);
        let too_big = policy.retain_bytes.is_some_and(|limit| kept_bytes > limit);
        if too_old || too_big {
            removed.push(path.clone());
            kept_bytes -= bytes;
        }
    }
    removed
}

// prune deletes the compressed segments past the retention limits; uncompressed ones wait for compression.
pub fn prune(dir: &Path, stem: &str, policy: &Policy) {
    let segments: Vec<(PathBuf, SystemTime, u64)> = list(dir, stem)
        .into_iter()
        .filter(|segment| segment.compressed)
        .filter_map(|segment| {
            let meta = fs::metadata(&segment.path).ok()?;
            Some((segment.path, meta.modified().ok()?, meta.len()))
        })
        .collect();
    for path in expired(&segments, policy, SystemTime::now()) {
        match fs::remove_file(&path) {
            Ok(()) => info!("retention removed {}", path.display()),
            Err(e) => warn!("retention remove {}: {e}", path.display()),
        }
    }
}

// Compressor compresses segments on its own thread, so a large one never delays a poll.
pub struct Compressor {
    queue: mpsc::Sender<PathBuf>,
}

impl Compressor {
    // start queues the segments a previous run left uncompressed and drops its partial outputs.
    pub fn start(dir: PathBuf, stem: String, policy: Policy) -> Self {
        let (queue, segments) = mpsc::channel::<PathBuf>();
        remove_parts(&dir, &stem);
        for segment in list(&dir, &stem).into_iter().filter(|s| !s.compressed) {
            let _ = queue.send(segment.path);
        }
        thread::spawn(move || {
            prune(&dir, &stem, &policy);
            for segment in segments {
                match compress(&segment) {
                    Ok(target) => info!("compressed {}", target.display()),
                    Err(e) => warn!("compress {}: {e}", segment.display()),
                }
                prune(&dir, &stem, &policy);
            }
        });
        Self { queue }
    }

    pub fn submit(&self, segment: PathBuf) {
        if self.queue.send(segment).is_err() {
            warn!("compressor stopped; segment left uncompressed");
        }
    }
}

fn remove_parts(dir: &Path, stem: &str) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(stem) && name.ends_with(PART_SUFFIX) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// open reads a dataset file, decompressing a .zst segment on the fly.
pub fn open(path: &Path) -> io::Result<Box<dyn BufRead>> {
    let file = File::open(path)?;
    if path.to_string_lossy().ends_with(COMPRESSED_SUFFIX) {
        return Ok(Box::new(BufReader::new(zstd::Decoder::new(file)?)));
    }
    Ok(Box::new(BufReader::new(file)))
}

// utc_stamp formats ms since the epoch as YYYYMMDDTHHMMSSZ, by the civil-from-days algorithm.
pub fn utc_stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, of_day) = (secs / SECS_PER_DAY, secs % SECS_PER_DAY);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    const STEM: &str = "croft-dataset";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("croft-segments-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stamps_are_utc() {
        assert_eq!(utc_stamp(1_792_478_649_836), "20261020T064409Z");
        assert_eq!(utc_stamp(951_782_400_000), "20000229T000000Z");
        assert_eq!(utc_stamp(0), "19700101T000000Z");
    }

    #[test]
    fn segment_names_round_trip_and_skip_other_files() {
        let name = segment_name(STEM, 1_792_478_649_836);
        assert_eq!(name, "croft-dataset-20261020T064409Z.jsonl");
        assert_eq!(
            parse_segment(STEM, &name),
            Some(("20261020T064409Z".into(), false))
        );
        assert_eq!(
            parse_segment(STEM, &format!("{name}.zst")),
            Some(("20261020T064409Z".into(), true))
        );
        assert!(is_segment_name(&name));
        for other in [
            "croft-dataset.jsonl",
            "croft-dataset-onset-only.jsonl",
            "croft-dataset-20261020T064409Z.jsonl.zst.part",
        ] {
            assert_eq!(parse_segment(STEM, other), None, "{other}");
            assert!(!is_segment_name(other), "{other}");
        }
    }

    #[test]
    fn rotation_is_due_by_age_or_size() {
        let policy = Policy {
            rotate_after: Some(Duration::from_secs(3600)),
            rotate_bytes: Some(1000),
            ..Policy::default()
        };
        assert!(!policy.is_due(Duration::from_secs(10), 10));
        assert!(policy.is_due(Duration::from_secs(3600), 10));
        assert!(policy.is_due(Duration::from_secs(10), 1000));
        assert!(!Policy::default().is_due(Duration::MAX, u64::MAX));
    }

    #[test]
    fn retention_drops_old_segments_then_oldest_past_the_size_cap() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * SECS_PER_DAY);
        let day = |n: u64| now - Duration::from_secs(n * SECS_PER_DAY);
        let segments = vec![
            (PathBuf::from("a"), day(40), 10),
            (PathBuf::from("b"), day(3), 50),
            (PathBuf::from("c"), day(2), 50),
            (PathBuf::from("d"), day(1), 50),
        ];
        let policy = Policy {
            retain: Some(Duration::from_secs(30 * SECS_PER_DAY)),
            retain_bytes: Some(100),
            ..Policy::default()
        };
        assert_eq!(
            expired(&segments, &policy, now),
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert!(expired(&segments, &Policy::default(), now).is_empty());
    }

    #[test]
    fn rotated_segment_compresses_and_reads_back() {
        let dir = scratch("compress");
        let live = dir.join("croft-dataset.jsonl");
        File::create(&live)
            .unwrap()
            .write_all(b"{\"unix_ms\":1}\n{\"unix_ms\":2}\n")
            .unwrap();
        let segment = rotate(&live, STEM, 1_792_478_649_836).unwrap();
        assert!(!live.exists());
        let target = compress(&segment).unwrap();
        assert!(!segment.exists());
        assert_eq!(
            list(&dir, STEM),
            vec![Segment {
                path: target.clone(),
                stamp: "20261020T064409Z".into(),
                compressed: true
            }]
        );
        let mut text = String::new();
        open(&target).unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "{\"unix_ms\":1}\n{\"unix_ms\":2}\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
