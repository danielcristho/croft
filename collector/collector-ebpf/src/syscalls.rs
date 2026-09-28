//! Counts every syscall by process and syscall number.

use aya_ebpf::{macros::tracepoint, programs::TracePointContext};
use collector_common::{DropSite, pack_key};

use crate::{
    context::{current_tgid, syscall_nr, tracepoint_ret},
    maps::{SYSCALL_BY_TYPE, count_on_err},
};

#[tracepoint]
pub fn sys_enter(ctx: TracePointContext) -> u32 {
    tracepoint_ret(count_syscall(&ctx))
}

fn count_syscall(ctx: &TracePointContext) -> Result<(), i32> {
    let key = pack_key(current_tgid(), syscall_nr(ctx)? as u32);
    // SAFETY: a racing update loses one count, it does not corrupt state.
    let count = unsafe { SYSCALL_BY_TYPE.get(key) }.copied().unwrap_or(0) + 1;
    count_on_err(
        SYSCALL_BY_TYPE.insert(key, count, 0),
        DropSite::SyscallCount,
    );
    Ok(())
}
