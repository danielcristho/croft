//! Seeds GPU_FDS with GPU fds opened before the collector attached.

use std::{
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use aya::Ebpf;
use collector_common::{PRESENT, is_gpu_path, pack_key};
use log::warn;

use crate::maps;

pub fn scan_gpu_fds(ebpf: &mut Ebpf) -> anyhow::Result<()> {
    let mut gpu_fds = maps::gpu_fds(ebpf)?;
    let mut refused = 0;
    for (tgid, fd) in open_gpu_fds() {
        if gpu_fds.insert(pack_key(tgid, fd), PRESENT, 0).is_err() {
            refused += 1;
        }
    }
    if refused > 0 {
        warn!("backfill could not record {refused} GPU fds");
    }
    Ok(())
}

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

fn points_at_gpu(fd_path: &Path) -> bool {
    fs::read_link(fd_path).is_ok_and(|target| is_gpu_path(target.as_os_str().as_bytes()))
}
