// Pre-populates GPU_FDS for processes that already had a GPU device open before
// this collector started, since their openat already happened and won't be seen.

use std::fs;

use aya::maps::HashMap;
use log::debug;

const GPU_PATH_PREFIXES: &[&str] = &["/dev/nvidia", "/dev/dri/"];

// scan_gpu_fds walks /proc/<pid>/fd, looking for fds already pointing at a GPU device.
pub fn scan_gpu_fds(ebpf: &mut aya::Ebpf) -> anyhow::Result<()> {
    let mut gpu_fds: HashMap<_, u64, u8> = HashMap::try_from(ebpf.map_mut("GPU_FDS").unwrap())?;

    let Ok(procs) = fs::read_dir("/proc") else {
        return Ok(());
    };
    for proc_entry in procs.flatten() {
        let Ok(tgid) = proc_entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = fs::read_dir(proc_entry.path().join("fd")) else {
            continue;
        };
        for fd_entry in fds.flatten() {
            let Ok(fd) = fd_entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(target) = fs::read_link(fd_entry.path()) else {
                continue;
            };
            let target = target.to_string_lossy();
            if !GPU_PATH_PREFIXES
                .iter()
                .any(|prefix| target.starts_with(prefix))
            {
                continue;
            }

            let key = ((tgid as u64) << 32) | fd as u64;
            if let Err(e) = gpu_fds.insert(key, 1u8, 0) {
                debug!("backfill: could not record tgid={tgid} fd={fd}: {e}");
            }
        }
    }
    Ok(())
}
