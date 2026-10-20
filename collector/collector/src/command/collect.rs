//! Pulls each node's dataset files from the collector DaemonSet into <out>/<node>/, appending only the bytes a local copy lacks.

use std::{
    cmp::Ordering,
    fs::{self, OpenOptions},
    path::Path,
};

use anyhow::{Context, bail};

use super::{external, stats::human_bytes};
use crate::{cli::CollectArgs, pipeline::segments};

// Consts for the kubectl selector for the collector pods, the chunk size for each pull, and the jsonpath for the pod and node names.
const SELECTOR: &str = "app=croft-collector";
const CHUNK_BYTES: u64 = 256 * 1024 * 1024; // Keep the chunk size small enough to avoid OOM on the collector pod.
const POD_AND_NODE: &str =
    "jsonpath={range .items[*]}{.metadata.name} {.spec.nodeName}{\"\\n\"}{end}";

struct Pod {
    name: String,
    node: String,
}

// execute collects the dataset from each collector pod, appending only the bytes the local copy lacks, and reports the result for each node.
pub fn execute(args: &CollectArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.out).with_context(|| format!("create {}", args.out.display()))?;
    let mut failed = 0;
    for pod in pods(&args.namespace)? {
        let local = args.out.join(&pod.node);
        match sync_node(args, &pod, &local) {
            Ok(report) => println!("{:<20} {report}", pod.node),
            Err(e) => {
                eprintln!("{:<20} {e:#}", pod.node);
                failed += 1;
            }
        }
    }
    if failed > 0 {
        bail!("collect from {failed} node(s)");
    }
    Ok(())
}

// pods lists the collector pods in the given namespace, returning their names and nodes.
fn pods(namespace: &str) -> anyhow::Result<Vec<Pod>> {
    let listing = external::output(
        "kubectl",
        &strings(&[
            "get",
            "pods",
            "-n",
            namespace,
            "-l",
            SELECTOR,
            "-o",
            POD_AND_NODE,
        ]),
    )?;
    Ok(listing
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(name, node)| Pod {
            name: name.to_string(),
            node: node.to_string(),
        })
        .collect())
}

// sync_node pulls the node's compressed segments and its live file; a segment still being compressed waits for the next collect.
fn sync_node(args: &CollectArgs, pod: &Pod, local_dir: &Path) -> anyhow::Result<String> {
    fs::create_dir_all(local_dir).with_context(|| format!("create {}", local_dir.display()))?;
    let remote = Path::new(&args.remote);
    let dir = remote.parent().unwrap_or(Path::new("/"));
    let live = remote
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let stem = live.strip_suffix(segments::LIVE_SUFFIX).unwrap_or(&live);
    let listing = external::output(
        "kubectl",
        &exec(args, pod, &["ls", "-1", &dir.to_string_lossy()]),
    )?;
    let names = listing.lines().filter(|name| {
        *name == live
            || segments::parse_segment(stem, name).is_some_and(|(_, compressed)| compressed)
    });
    let (mut files, mut gained) = (0, 0);
    for name in names {
        gained += sync(args, pod, &dir.join(name), &local_dir.join(name))?;
        files += 1;
    }
    Ok(format!("+{} over {files} file(s)", human_bytes(gained)))
}

// sync appends like rsync --append: a segment only grows until it is closed; a live file smaller than the copy was rotated, so it starts over.
fn sync(args: &CollectArgs, pod: &Pod, remote: &Path, local: &Path) -> anyhow::Result<u64> {
    let remote_bytes = remote_size(args, pod, remote)?;
    let mut have = local_size(local);
    match remote_bytes.cmp(&have) {
        Ordering::Equal => return Ok(0),
        Ordering::Less => {
            fs::remove_file(local).with_context(|| format!("remove {}", local.display()))?;
            have = 0;
        }
        Ordering::Greater => {}
    }
    pull(args, pod, remote, local, remote_bytes)?;
    Ok(remote_bytes - have)
}

// pull resumes each chunk from the bytes that arrived, and gives up when a chunk brings none.
fn pull(
    args: &CollectArgs,
    pod: &Pod,
    remote: &Path,
    local: &Path,
    remote_bytes: u64,
) -> anyhow::Result<()> {
    let mut have = local_size(local);
    while have < remote_bytes {
        let result = append(
            args,
            pod,
            remote,
            local,
            have,
            CHUNK_BYTES.min(remote_bytes - have),
        );
        let now = local_size(local);
        if now == have {
            return result.and_then(|()| bail!("no bytes from {}", pod.name));
        }
        have = now;
    }
    Ok(())
}

// local_size returns the size of the local file, or 0 if it does not exist.
fn local_size(local: &Path) -> u64 {
    fs::metadata(local).map_or(0, |meta| meta.len())
}

// remote_size returns the size of the remote file, or an error if it cannot be read.
fn remote_size(args: &CollectArgs, pod: &Pod, remote: &Path) -> anyhow::Result<u64> {
    let remote = remote.to_string_lossy();
    let size = external::output(
        // Use kubectl exec to run stat in the collector pod,
        // and read the size of the remote file.
        "kubectl",
        &exec(args, pod, &["stat", "-c", "%s", &remote]),
    )?;
    size.trim()
        .parse()
        .with_context(|| format!("read size of {remote}"))
}

// append stops at the size read before, so a row the collector is writing arrives whole on the next collect.
fn append(
    args: &CollectArgs,
    pod: &Pod,
    remote: &Path,
    local: &Path,
    skip: u64,
    count: u64,
) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(local)
        .with_context(|| format!("open {}", local.display()))?;
    let dd = [
        "dd".to_string(),
        format!("if={}", remote.display()),
        "iflag=skip_bytes,count_bytes".into(),
        format!("skip={skip}"),
        format!("count={count}"),
        "bs=1M".into(),
        "status=none".into(),
    ];
    let dd: Vec<&str> = dd.iter().map(String::as_str).collect();
    external::to_file("kubectl", &exec(args, pod, &dd), file)
}

// exec builds the kubectl exec command to run the given command in the collector pod.
fn exec(args: &CollectArgs, pod: &Pod, command: &[&str]) -> Vec<String> {
    let mut exec = strings(&["exec", "-n", &args.namespace, &pod.name, "--"]);
    exec.extend(strings(command));
    exec
}

// strings converts a slice of &str to a Vec<String>.
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}
