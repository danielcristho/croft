//! Tracks GPU device fds and counts the ioctls a process sends through them.

use aya_ebpf::{
    helpers::{bpf_get_current_pid_tgid, bpf_ktime_get_ns, bpf_probe_read_user_buf},
    macros::tracepoint,
    programs::TracePointContext,
};
use collector_common::{DropSite, GpuActivity, PRESENT, is_gpu_path, pack_key};

use crate::{
    context::{arg, current_tgid, ret, tracepoint_ret},
    maps::{GPU_ACTIVITY, GPU_FDS, PENDING_GPU_OPEN, count_on_err},
};

const PATH_PREFIX_LEN: usize = 16;
const FD_ARG: usize = 0;
const FILENAME_ARG: usize = 1;

#[tracepoint]
pub fn sys_enter_openat(ctx: TracePointContext) -> u32 {
    tracepoint_ret(mark_gpu_open(&ctx))
}

#[tracepoint]
pub fn sys_exit_openat(ctx: TracePointContext) -> u32 {
    tracepoint_ret(record_gpu_fd(&ctx))
}

#[tracepoint]
pub fn sys_enter_ioctl(ctx: TracePointContext) -> u32 {
    tracepoint_ret(record_gpu_ioctl(&ctx))
}

#[tracepoint]
pub fn sys_enter_close(ctx: TracePointContext) -> u32 {
    tracepoint_ret(forget_gpu_fd(&ctx))
}

fn mark_gpu_open(ctx: &TracePointContext) -> Result<(), i32> {
    let filename: u64 = arg(ctx, FILENAME_ARG)?;
    let mut buf = [0u8; PATH_PREFIX_LEN];
    // SAFETY: a bad user pointer makes the helper return an error, not fault.
    if unsafe { bpf_probe_read_user_buf(filename as *const u8, &mut buf) }.is_err() {
        return Ok(());
    }
    if !is_gpu_path(&buf) {
        return Ok(());
    }
    let inserted = PENDING_GPU_OPEN.insert(bpf_get_current_pid_tgid(), PRESENT, 0);
    count_on_err(inserted, DropSite::PendingOpen);
    Ok(())
}

fn record_gpu_fd(ctx: &TracePointContext) -> Result<(), i32> {
    if PENDING_GPU_OPEN.remove(bpf_get_current_pid_tgid()).is_err() {
        return Ok(());
    }
    let fd = ret(ctx)?;
    if fd < 0 {
        return Ok(());
    }
    let inserted = GPU_FDS.insert(pack_key(current_tgid(), fd as u32), PRESENT, 0);
    count_on_err(inserted, DropSite::GpuFd);
    Ok(())
}

fn record_gpu_ioctl(ctx: &TracePointContext) -> Result<(), i32> {
    let fd: u64 = arg(ctx, FD_ARG)?;
    let tgid = current_tgid();
    // SAFETY: the reference is checked for presence and dropped on this line.
    if unsafe { GPU_FDS.get(pack_key(tgid, fd as u32)) }.is_none() {
        return Ok(());
    }
    // SAFETY: the helper takes no arguments and cannot fault.
    let now = unsafe { bpf_ktime_get_ns() };
    // SAFETY: a racing update loses one count, it does not corrupt state.
    let activity = GpuActivity::record(unsafe { GPU_ACTIVITY.get(tgid) }, now);
    count_on_err(
        GPU_ACTIVITY.insert(tgid, activity, 0),
        DropSite::GpuActivity,
    );
    Ok(())
}

fn forget_gpu_fd(ctx: &TracePointContext) -> Result<(), i32> {
    let fd: u64 = arg(ctx, FD_ARG)?;
    let _ = GPU_FDS.remove(pack_key(current_tgid(), fd as u32));
    Ok(())
}
