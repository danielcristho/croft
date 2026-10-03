//! Maps shared with userspace, and the counter for inserts they refuse.

use aya_ebpf::{
    macros::map,
    maps::{HashMap, LruHashMap, PerCpuArray},
};
use collector_common::{DropSite, GpuActivity};

// Consts for the maximum number of entries in the eBPF maps.
const MAX_TRACKED: u32 = 10240;
const MAX_SYSCALL_ENTRIES: u32 = 65536;

#[map]
pub static PENDING_GPU_OPEN: HashMap<u64, u8> = HashMap::with_max_entries(MAX_TRACKED, 0);

// LRU, so fds of exited processes age out.
#[map]
pub static GPU_FDS: LruHashMap<u64, u8> = LruHashMap::with_max_entries(MAX_TRACKED, 0);

#[map]
pub static GPU_ACTIVITY: HashMap<u32, GpuActivity> = HashMap::with_max_entries(MAX_TRACKED, 0);

#[map]
pub static SYSCALL_BY_TYPE: LruHashMap<u64, u64> =
    LruHashMap::with_max_entries(MAX_SYSCALL_ENTRIES, 0);

// Per-CPU so the hot path increments without atomics; userspace sums the CPUs.
#[map]
pub static DROPS: PerCpuArray<u64> = PerCpuArray::with_max_entries(DropSite::ALL.len() as u32, 0);

pub fn count_on_err<E>(result: Result<(), E>, site: DropSite) {
    if result.is_ok() {
        return;
    }
    if let Some(slot) = DROPS.get_ptr_mut(site.index()) {
        // SAFETY: map-owned per-CPU slot; a nested program on this CPU can lose one count, not corrupt memory.
        unsafe { *slot += 1 };
    }
}
