//! Clears per-process state when a process exits.

use aya_ebpf::{macros::tracepoint, programs::TracePointContext};

use crate::{
    context::{current_is_group_leader, current_tgid},
    maps::GPU_ACTIVITY,
};

#[tracepoint]
pub fn sched_process_exit(_ctx: TracePointContext) -> u32 {
    if current_is_group_leader() {
        let _ = GPU_ACTIVITY.remove(current_tgid());
    }
    0
}
