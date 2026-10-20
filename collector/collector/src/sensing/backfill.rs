//! Seeds GPU_FDS with GPU fds opened before the collector attached, and marks their processes as holding a GPU.

use std::{
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use aya::Ebpf;
use collector_common::{GpuActivity, PRESENT, is_gpu_path, pack_key};
use log::warn;

use crate::sensing::maps;

// scan_gpu_fds scans /proc for open GPU fds and inserts them into the eBPF GPU_FDS and GPU_ACTIVITY maps.
pub fn scan_gpu_fds(ebpf: &mut Ebpf) -> anyhow::Result<()> {
    let open: Vec<(u32, u32)> = open_gpu_fds().collect();
    let mut refused = 0;
    let mut gpu_fds = maps::gpu_fds(ebpf)?;
    for &(tgid, fd) in &open {
        if gpu_fds.insert(pack_key(tgid, fd), PRESENT, 0).is_err() {
            refused += 1;
        }
    }
    // A CUDA process submits work without ioctls once set up, so a held fd is the only sign it uses the GPU.
    let mut gpu_activity = maps::gpu_activity_mut(ebpf)?;
    for tgid in held_tgids(&open) {
        if gpu_activity.insert(tgid, HELD_BEFORE_ATTACH, 0).is_err() {
            refused += 1;
        }
    }
    if refused > 0 {
        warn!("backfill could not record {refused} GPU fds or processes");
    }
    Ok(())
}

// HELD_BEFORE_ATTACH marks a process that held a GPU fd at attach; no ioctl seen, so it reads as gpu-held.
const HELD_BEFORE_ATTACH: GpuActivity = GpuActivity {
    count: 0,
    last_ns: 0,
};

// held_tgids returns each process in the (tgid, fd) pairs once, in order.
fn held_tgids(open: &[(u32, u32)]) -> Vec<u32> {
    let mut tgids: Vec<u32> = open.iter().map(|&(tgid, _)| tgid).collect();
    tgids.sort_unstable();
    tgids.dedup();
    tgids
}

// open_gpu_fds returns an iterator over the (tgid, fd) pairs of all open GPU fds in the system.
fn open_gpu_fds() -> impl Iterator<Item = (u32, u32)> {
    numeric_entries(PathBuf::from("/proc")).flat_map(|(tgid, proc_dir)| {
        numeric_entries(proc_dir.join("fd"))
            .filter(|(_, fd_path)| points_at_gpu(fd_path))
            .map(move |(fd, _)| (tgid, fd))
    })
}

// numeric_entries yields the entries of dir named by a number, such as pids and fds.
fn numeric_entries(dir: PathBuf) -> impl Iterator<Item = (u32, PathBuf)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let number = entry.file_name().to_str()?.parse().ok()?;
            Some((number, entry.path()))
        })
}

// points_at_gpu returns true if the fd_path is a symlink to a GPU device.
fn points_at_gpu(fd_path: &Path) -> bool {
    fs::read_link(fd_path).is_ok_and(|target| is_gpu_path(target.as_os_str().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_tgids_lists_each_process_once() {
        assert_eq!(held_tgids(&[(7, 3), (2, 5), (7, 4)]), vec![2, 7]);
    }
}
