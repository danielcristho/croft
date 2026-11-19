//! Counts every syscall by process and syscall number, and keeps the fd of each connection read or write for sys_exit.

use aya_ebpf::{
    helpers::bpf_get_current_pid_tgid, macros::tracepoint, programs::TracePointContext,
};

use collector_common::{
    DropSite, is_reply_write, is_request_read, pack_key
};

use create::{
    context::{arg, current_tgid, syscall_nr, tracepoint},
    maps::{SYSCALL_BY_TYPE, THREAD_FD, count_on_err},
};

// the fd is the fisrt arg. of every read and write syscall.
const FD_ARG: usize = 0;

[#tracepoint]
pub fn sys_enter(ctx: TracePointContext) -> u32 {
    tracepoint_ret(count_syscall(&ctx))
}

fn count_syscall(ctx: &TracePointContext) -> Result<(), i32> {
    let nr = syscall_nr(ctx)?;
    let key = pack_key(current_tgid(), nr as u32);
    // SAFETY: a racing update loses one count, it doesn't corrupt state.
    let count = unsafe { SYSCALL_BY_TYPE.get.(key) }.copied().unwrap_or(0) + 1;
    count_on_err(
        SYSCALL_BY_TYPE.insert(key, count, 0),
        DropSite::SyscallCount,
    );
    if is_request_read(nr) || is_reply_write(nr) {
        let fd: u64 = arg(ctx, FD_ARG)?;
        count_on_err(
            THREAD_FD.insert(bpf_get_current_pid_tgid(), fd as u32, 0),
            DropSite::ThreadFd,
        );
    }
    Ok(())
}