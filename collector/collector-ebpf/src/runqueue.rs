//! Time threads wait on a run queue for a CPU, summed per process ("rq time" in Landau et al., 2026).

use aya_ebpf::{helpers::bpf_ktime_get_ns, macros::tracepoint, programs::TracePointContext};
use collector_common::DropSite;

use crate::{
    context::{current_tgid, tracepoint_ret},
    maps::{RUNQUEUE_NS, RUNQUEUE_PENDING, RUNQUEUE_SINCE, count_on_err},
};

// Consts for the offsets of the fields in the fixed sched tracepoint records.
const WAKEUP_PID_OFFSET: usize = 24;
const PREV_PID_OFFSET: usize = 24;
const PREV_STATE_OFFSET: usize = 32;
const NEXT_PID_OFFSET: usize = 56;
// prev_state holds the reported task state in its low byte; zero there means still runnable (yielded or preempted).
const STATE_MASK: i64 = 0xff;
const IDLE_TID: u32 = 0;

#[tracepoint]
pub fn sched_wakeup(ctx: TracePointContext) -> u32 {
    tracepoint_ret(mark_woken(&ctx))
}

#[tracepoint]
pub fn sched_wakeup_new(ctx: TracePointContext) -> u32 {
    tracepoint_ret(mark_woken(&ctx))
}

#[tracepoint]
pub fn sched_switch(ctx: TracePointContext) -> u32 {
    tracepoint_ret(account_switch(&ctx))
}

fn mark_woken(ctx: &TracePointContext) -> Result<(), i32> {
    // SAFETY: offset is inside the fixed sched_wakeup record.
    let tid: i32 = unsafe { ctx.read_at(WAKEUP_PID_OFFSET) }?;
    // SAFETY: the helper takes no arguments and cannot fault.
    enqueue(tid as u32, unsafe { bpf_ktime_get_ns() });
    Ok(())
}

// account_switch runs as prev: it settles prev's wait, requeues prev if still runnable, and ends next's wait.
fn account_switch(ctx: &TracePointContext) -> Result<(), i32> {
    // SAFETY: offsets are inside the fixed sched_switch record.
    let (prev, state, next): (i32, i64, i32) = unsafe {
        (
            ctx.read_at(PREV_PID_OFFSET)?,
            ctx.read_at(PREV_STATE_OFFSET)?,
            ctx.read_at(NEXT_PID_OFFSET)?,
        )
    };
    // SAFETY: the helper takes no arguments and cannot fault.
    let now = unsafe { bpf_ktime_get_ns() };
    settle(prev as u32);
    if state & STATE_MASK == 0 {
        enqueue(prev as u32, now);
    }
    start_running(next as u32, now);
    Ok(())
}

fn enqueue(tid: u32, now: u64) {
    if tid == IDLE_TID {
        return;
    }
    count_on_err(RUNQUEUE_SINCE.insert(tid, now, 0), DropSite::RunqueueSince);
}

// start_running parks the wait by thread id: next's process is not known until next is the current task.
fn start_running(tid: u32, now: u64) {
    // SAFETY: the value is copied out on this line.
    let Some(since) = (unsafe { RUNQUEUE_SINCE.get(tid) }).copied() else {
        return;
    };
    let _ = RUNQUEUE_SINCE.remove(tid);
    // SAFETY: a racing update loses one wait, it does not corrupt state.
    let pending = unsafe { RUNQUEUE_PENDING.get(tid) }.copied().unwrap_or(0);
    count_on_err(
        RUNQUEUE_PENDING.insert(tid, pending + now.saturating_sub(since), 0),
        DropSite::RunqueuePending,
    );
}

// settle charges a thread's parked wait to its process while it is the current task.
fn settle(tid: u32) {
    // SAFETY: the value is copied out on this line.
    let Some(waited) = (unsafe { RUNQUEUE_PENDING.get(tid) }).copied() else {
        return;
    };
    let _ = RUNQUEUE_PENDING.remove(tid);
    let tgid = current_tgid();
    // SAFETY: a racing update loses one wait, it does not corrupt state.
    let total = unsafe { RUNQUEUE_NS.get(tgid) }.copied().unwrap_or(0) + waited;
    count_on_err(RUNQUEUE_NS.insert(tgid, total, 0), DropSite::RunqueueNs);
}
