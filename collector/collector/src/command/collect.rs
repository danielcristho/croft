//! Pulls each node's dataset from the collector DaemonSet, appending only the bytes the local copy lacks.

use std::{
    cmp::Ordering,
    fs::{self, OpenOptions},
    path::Path,
};

use anyhow::{Context, bail};

use super::{external, stats::human_bytes};
use crate::cli::CollectArgs;

// Consts for the kubectl selector for the collector pods, the chunk size for each pull, and the jsonpath for the pod and node names.
const SELECTOR: &str = "app=croft-collector";
const CHUNK_BYTES: u64 = 256 * 1024 * 1024; // Keep the chunk size small enough to avoid OOM on the collector pod.
const POD_AND_NODE: &str =
    "jsonpath={range .items[*]}{.metadata.name} {.spec.nodeName}{\"\\n\"}{end}";

struct Pod {
    name: String,
    node: String,
}

pub fn execute(args: &CollectArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.out).with_context(|| format!("create {}", args.out.display()))?;
    let mut failed = 0;
    for pod in pods(&args.namespace)? {
        let local = args.out.join(format!("{}.jsonl", pod.node));
        match sync(args, &pod, &local) {
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

// sync appends like rsync --append: a dataset only grows, so the local file is a prefix of the remote one.
fn sync(args: &CollectArgs, pod: &Pod, local: &Path) -> anyhow::Result<String> {
    let have = local_size(local);
    let remote = remote_size(args, pod)?;
    match remote.cmp(&have) {
        Ordering::Equal => Ok("up to date".into()),
        Ordering::Less => bail!(
            "remote has {} but the local copy {}; move {} aside to start over",
            human_bytes(remote),
            human_bytes(have),
            local.display()
        ),
        Ordering::Greater => {
            pull(args, pod, local, remote)?;
            Ok(format!(
                "+{}, {} total",
                human_bytes(remote - have),
                human_bytes(remote)
            ))
        }
    }
}

// pull resumes each chunk from the bytes that arrived, and gives up when a chunk brings none.
fn pull(args: &CollectArgs, pod: &Pod, local: &Path, remote: u64) -> anyhow::Result<()> {
    let mut have = local_size(local);
    while have < remote {
        let result = append(args, pod, local, have, CHUNK_BYTES.min(remote - have));
        let now = local_size(local);
        if now == have {
            return result.and_then(|()| bail!("no bytes from {}", pod.name));
        }
        have = now;
    }
    Ok(())
}

fn local_size(local: &Path) -> u64 {
    fs::metadata(local).map_or(0, |meta| meta.len())
}

fn remote_size(args: &CollectArgs, pod: &Pod) -> anyhow::Result<u64> {
    let size = external::output(
        "kubectl",
        &exec(args, pod, &["stat", "-c", "%s", &args.remote]),
    )?;
    size.trim()
        .parse()
        .with_context(|| format!("read size of {}", args.remote))
}

// append stops at the size read before, so a row the collector is writing arrives whole on the next collect.
fn append(
    args: &CollectArgs,
    pod: &Pod,
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
        format!("if={}", args.remote),
        "iflag=skip_bytes,count_bytes".into(),
        format!("skip={skip}"),
        format!("count={count}"),
        "bs=1M".into(),
        "status=none".into(),
    ];
    let dd: Vec<&str> = dd.iter().map(String::as_str).collect();
    external::to_file("kubectl", &exec(args, pod, &dd), file)
}

fn exec(args: &CollectArgs, pod: &Pod, command: &[&str]) -> Vec<String> {
    let mut exec = strings(&["exec", "-n", &args.namespace, &pod.name, "--"]);
    exec.extend(strings(command));
    exec
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}
