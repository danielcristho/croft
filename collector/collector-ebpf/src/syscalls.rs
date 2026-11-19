//! Counts every syscall by process and syscall number, and keeps the fd of each read or write on an accepted connection for sys_exit.

use aya_ebpf::{
    helpers::bpf_get_current_pid_tgid, macros::tracepoint, programs::TracePointContext,
};
use collector_common::{DropSite, is_reply_write, is_request_read, pack_key};

use crate::{
    context::{FD_ARG, arg, current_tgid, syscall_nr, tracepoint_ret},
    maps::{SERVER_FDS, SYSCALL_BY_TYPE, THREAD_FD, count_on_err},
};

#[tracepoint]
pub fn sys_enter(ctx: TracePointContext) -> u32 {
    tracepoint_ret(count_syscall(&ctx))
}

fn count_syscall(ctx: &TracePointContext) -> Result<(), i32> {
    let nr = syscall_nr(ctx)?;
    let key = pack_key(current_tgid(), nr as u32);
    // SAFETY: a racing update loses one count, it does not corrupt state.
    let count = unsafe { SYSCALL_BY_TYPE.get(&key) }.copied().unwrap_or(0) + 1;
    count_on_err(
        SYSCALL_BY_TYPE.insert(key, count, 0),
        DropSite::SyscallCount,
    );
    if is_request_read(nr) || is_reply_write(nr) {
        let fd = arg::<u64>(ctx, FD_ARG)? as u32;
        // SAFETY: the reference is checked for presence and dropped on this line.
        if unsafe { SERVER_FDS.get(&pack_key(current_tgid(), fd)) }.is_none() {
            return Ok(());
        }
        count_on_err(
            THREAD_FD.insert(bpf_get_current_pid_tgid() as u32, fd, 0),
            DropSite::ThreadFd,
        );
    }
    Ok(())
}
