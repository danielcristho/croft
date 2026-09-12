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
const SYSCALL_NR_OFFSET: usize = 8;
const GPU_PATH_PREFIXES: &[&[u8]] = &[b"/dev/nvidia", b"/dev/dri/"];
const MAX_TRACKED: u32 = 10240;
// Higher cardinality than the other maps: one entry per (tgid, syscall number) pair.
const MAX_SYSCALL_ENTRIES: u32 = 65536;

#[map]
static PENDING_GPU_OPEN: HashMap<u64, u8> = HashMap::with_max_entries(MAX_TRACKED, 0);
#[map]
static GPU_FDS: LruHashMap<u64, u8> = LruHashMap::with_max_entries(MAX_TRACKED, 0);
#[map]
static GPU_ACTIVITY: HashMap<u32, GpuActivity> = HashMap::with_max_entries(MAX_TRACKED, 0);
#[map]
static SYSCALL_BY_TYPE: LruHashMap<u64, u64> = LruHashMap::with_max_entries(MAX_SYSCALL_ENTRIES, 0);

// pack_key packs a tgid and a per-tgid id (a fd or a syscall number) into one map key.
fn pack_key(tgid: u32, id: u32) -> u64 {
    ((tgid as u64) << 32) | id as u64
}

// current_tgid returns the thread group id (the process id) of the caller.
fn current_tgid() -> u32 {
    (bpf_get_current_pid_tgid() >> 32) as u32
}

// is_gpu_path checks a probe-read buffer against the paths that mark a GPU device node.
fn is_gpu_path(buf: &[u8]) -> bool {
    GPU_PATH_PREFIXES
        .iter()
        .any(|prefix| buf.starts_with(prefix))
}

// Tracepoints
#[tracepoint]
pub fn sys_enter_openat(ctx: TracePointContext) -> u32 {
    try_enter_openat(ctx).unwrap_or(0)
}

// try_enter_openat checks if the openat is for a GPU device node and if so, records the pid_tgid in PENDING_GPU_OPEN.
fn try_enter_openat(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed sys_enter_openat record.
    let filename_ptr: u64 = unsafe { ctx.read_at(ARGS_OFFSET + 8)? };

    let mut buf = [0u8; 16];
    // SAFETY: faults safely if filename_ptr is not a mapped user address.
    if unsafe { bpf_probe_read_user_buf(filename_ptr as *const u8, &mut buf) }.is_err() {
        return Ok(0);
    }
    if !is_gpu_path(&buf) {
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
    // SAFETY: a racing remove would only miss this pid_tgid, not corrupt state.
    if unsafe { PENDING_GPU_OPEN.get(&pid_tgid) }.is_none() {
        return Ok(0);
    }
    let _ = PENDING_GPU_OPEN.remove(&pid_tgid);

    // SAFETY: offset is inside the fixed sys_exit_openat record.
    let ret: i64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    if ret < 0 {
        return Ok(0);
    }

    let tgid = current_tgid();
    let _ = GPU_FDS.insert(&pack_key(tgid, ret as u32), &1u8, 0);
    Ok(0)
}

// sys_enter_ioctl checks if the ioctl is for a GPU fd and if so, records the activity in GPU_ACTIVITY.
#[tracepoint]
pub fn sys_enter_ioctl(ctx: TracePointContext) -> u32 {
    try_enter_ioctl(ctx).unwrap_or(0)
}

fn try_enter_ioctl(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed sys_enter_ioctl record.
    let fd: u64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    let tgid = current_tgid();

    // SAFETY: a racing remove would only miss this fd, not corrupt state.
    if unsafe { GPU_FDS.get(&pack_key(tgid, fd as u32)) }.is_none() {
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
    // SAFETY: offset is inside the fixed sys_enter_close record.
    let fd: u64 = unsafe { ctx.read_at(ARGS_OFFSET)? };
    let tgid = current_tgid();
    let _ = GPU_FDS.remove(&pack_key(tgid, fd as u32));
    Ok(0)
}

// sys_enter counts every syscall by (tgid, syscall number).
#[tracepoint]
pub fn sys_enter(ctx: TracePointContext) -> u32 {
    try_enter_syscall(ctx).unwrap_or(0)
}

fn try_enter_syscall(ctx: TracePointContext) -> Result<u32, i32> {
    // SAFETY: offset is inside the fixed raw_syscalls/sys_enter record.
    let syscall_nr: i64 = unsafe { ctx.read_at(SYSCALL_NR_OFFSET)? };
    let key = pack_key(current_tgid(), syscall_nr as u32);

    // SAFETY: a racing update would only lose one count, not corrupt state.
    let count = unsafe { SYSCALL_BY_TYPE.get(&key) }.copied().unwrap_or(0) + 1;
    let _ = SYSCALL_BY_TYPE.insert(&key, &count, 0);
    Ok(0)
}

// sys_enter_exit_group removes all GPU_FDS and GPU_ACTIVITY for the exiting process.
#[tracepoint]
pub fn sys_enter_exit_group(ctx: TracePointContext) -> u32 {
    try_enter_exit_group(ctx).unwrap_or(0)
}

fn try_enter_exit_group(_ctx: TracePointContext) -> Result<u32, i32> {
    let _ = GPU_ACTIVITY.remove(&current_tgid());
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
