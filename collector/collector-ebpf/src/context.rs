//! Reads from the current task and from fixed tracepoint records.

use aya_ebpf::{helpers::bpf_get_current_pid_tgid, programs::TracePointContext};

// Consts for the offsets of the fields in the fixed tracepoint records.
const ARGS_OFFSET: usize = 16;
const RET_OFFSET: usize = 16;
const SYSCALL_NR_OFFSET: usize = 8;
const ARG_SIZE: usize = 8;

// The fd is the first argument of every fd syscall: read, write, ioctl, close.
pub const FD_ARG: usize = 0;

pub fn current_tgid() -> u32 {
    (bpf_get_current_pid_tgid() >> 32) as u32
}

pub fn current_is_group_leader() -> bool {
    let pid_tgid = bpf_get_current_pid_tgid();
    pid_tgid as u32 == (pid_tgid >> 32) as u32
}

pub fn arg<T>(ctx: &TracePointContext, index: usize) -> Result<T, i32> {
    // SAFETY: callers pass an index below six, inside the fixed sys_enter record.
    unsafe { ctx.read_at(ARGS_OFFSET + index * ARG_SIZE) }
}

pub fn ret(ctx: &TracePointContext) -> Result<i64, i32> {
    // SAFETY: offset is inside the fixed sys_exit record.
    unsafe { ctx.read_at(RET_OFFSET) }
}

pub fn syscall_nr(ctx: &TracePointContext) -> Result<i64, i32> {
    // SAFETY: offset is inside the fixed raw_syscalls/sys_enter record.
    unsafe { ctx.read_at(SYSCALL_NR_OFFSET) }
}

// tracepoint_ret maps a handler result to 0, the return value the kernel expects either way.
pub fn tracepoint_ret<E>(_result: Result<(), E>) -> u32 {
    0
}
