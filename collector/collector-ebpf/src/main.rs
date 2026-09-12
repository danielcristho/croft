#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_get_current_pid_tgid, bpf_ktime_get_ns, bpf_probe_read_user_buf},
    macros::{map, tracepoint},
    maps::{HashMap, LruHashMap},
    programs::TracePointContext,
};
use collector_common::GpuActivity;

// Consts
const ARGS_OFFSET: usize = 16;
const GPU_PATH_PREFIXES: &[&[u8]] = &[b"/dev/nvidia", b"/dev/dri/"];
const MAX_TRACKED: u32 = 10240;

#[map]
static PENDING_GPU_OPEN: HashMap<u64, u8> = HashMap::with_max_entries(MAX_TRACKED, 0);
// ponytail: LRU, not exact per-tgid cleanup on exit; a stale (tgid, fd) can outlive its
// process until eviction. Switch to per-tgid enumeration if reused tgids misfire in practice.
#[map]
static GPU_FDS: LruHashMap<u64, u8> = LruHashMap::with_max_entries(MAX_TRACKED, 0);
#[map]
static GPU_ACTIVITY: HashMap<u32, GpuActivity> = HashMap::with_max_entries(MAX_TRACKED, 0);

// fd_key returns a u64 key for the GPU_FDS map from a tgid and fd.
fn fd_key(tgid: u32, fd: u32) -> u64 {
    ((tgid as u64) << 32) | fd as u64
}

// Tracepoints
#[tracepoint]
pub fn sys_enter_openat(ctx: TracePointContext) -> u32 {
    try_enter_openat(ctx).unwrap_or(0)
}

// try_enter_openat checks if the openat is for a GPU device node and if so, records the pid_tgid in PENDING_GPU_OPEN.
fn try_enter_openat(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed sys_enter_openat record; filename is args[1].
    let filename_ptr: u64 = unsafe { ctx.read_at(ARGS_OFFSET + 8)? };

    let mut buf = [0u8; 16];
    // SAFETY: faults safely if filename_ptr is not a mapped user address.
    if unsafe { bpf_probe_read_user_buf(filename_ptr as *const u8, &mut buf) }.is_err() {
        return Ok(0);
    }
    if !GPU_PATH_PREFIXES
        .iter()
        .any(|prefix| buf.starts_with(prefix))
    {
        return Ok(0);
    }
    let pid_tgid = bpf_get_current_pid_tgid();
    let _ = PENDING_GPU_OPEN.insert(&pid_tgid, &1u8, 0);
    Ok(0)
}

// sys_exit_openat checks if the openat was for a GPU device node and if so, records the fd in GPU_FDS.
#[tracepoint]
pub fn sys_exit_openat(ctx: TracePointContext) -> u32 {
    try_exit_openat(ctx).unwrap_or(0)
}

// try_exit_openat checks if the openat was for a GPU device node and if so, records the fd in GPU_FDS.
fn try_exit_openat(ctx: TracePointContext) -> Result<u32, i32> {
    let pid_tgid = bpf_get_current_pid_tgid();
    // SAFETY: a racing remove would only miss this fd, not corrupt state.
    if unsafe { PENDING_GPU_OPEN.get(&pid_tgid) }.is_none() {
        return Ok(0);
    }
    let _ = PENDING_GPU_OPEN.remove(&pid_tgid);

    // SAFETY: offset is inside the fixed sys_exit_openat record.
    let ret: i64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    if ret < 0 {
        return Ok(0);
    }

    let tgid = (pid_tgid >> 32) as u32;
    let _ = GPU_FDS.insert(&fd_key(tgid, ret as u32), &1u8, 0);
    Ok(0)
}

// sys_enter_ioctl checks if the ioctl is for a GPU fd and if so, records the activity in GPU_ACTIVITY.
#[tracepoint]
pub fn sys_enter_ioctl(ctx: TracePointContext) -> u32 {
    try_enter_ioctl(ctx).unwrap_or(0)
}

fn try_enter_ioctl(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed sys_enter_ioctl record; fd is args[0].
    let fd: u64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    let pid_tgid = bpf_get_current_pid_tgid();
    let tgid = (pid_tgid >> 32) as u32;

    // SAFETY: a racing remove would only miss this fd, not corrupt state.
    if unsafe { GPU_FDS.get(&fd_key(tgid, fd as u32)) }.is_none() {
        return Ok(0);
    }

    // SAFETY: no arguments, cannot fault.
    let now = unsafe { bpf_ktime_get_ns() };
    // SAFETY: a racing update would only lose one count, not corrupt state.
    let activity = match unsafe { GPU_ACTIVITY.get(&tgid) } {
        Some(prev) => GpuActivity {
            count: prev.count + 1,
            last_ns: now,
        },
        None => GpuActivity {
            count: 1,
            last_ns: now,
        },
    };
    let _ = GPU_ACTIVITY.insert(&tgid, &activity, 0);
    Ok(0)
}

// sys_enter_close checks if the close is for a GPU fd and if so, removes the fd from GPU_FDS.
#[tracepoint]
pub fn sys_enter_close(ctx: TracePointContext) -> u32 {
    try_enter_close(ctx).unwrap_or(0)
}

fn try_enter_close(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed sys_enter_close record; fd is args[0].
    let fd: u64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    let pid_tgid = bpf_get_current_pid_tgid();
    let tgid = (pid_tgid >> 32) as u32;
    let _ = GPU_FDS.remove(&fd_key(tgid, fd as u32));
    Ok(0)
}

// sys_enter_exit_group fires once for the whole process (not per thread), unlike
// sched_process_exit; drops the exiting tgid's activity so a dead process stops
// being reported.
#[tracepoint]
pub fn sys_enter_exit_group(ctx: TracePointContext) -> u32 {
    try_enter_exit_group(ctx).unwrap_or(0)
}

fn try_enter_exit_group(_ctx: TracePointContext) -> Result<u32, i32> {
    let tgid = (bpf_get_current_pid_tgid() >> 32) as u32;
    let _ = GPU_ACTIVITY.remove(&tgid);
    Ok(0)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
