//! Typed access to the eBPF maps by their shared names.

use anyhow::Context as _;
use aya::{
    Ebpf,
    maps::{HashMap, Map, MapData, PerCpuArray},
};
use collector_common::{ConnectKey, GpuActivity, map_names};

// find returns a reference to the map with the given name, or an error if it is not present.
fn find<'a>(ebpf: &'a Ebpf, name: &str) -> anyhow::Result<&'a Map> {
    ebpf.map(name)
        .with_context(|| format!("map {name} not in the eBPF object"))
}

// gpu_activity returns a typed view of the GPU activity map.
pub fn gpu_activity(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u32, GpuActivity>> {
    Ok(HashMap::try_from(find(ebpf, map_names::GPU_ACTIVITY)?)?)
}

// syscall_counts returns a typed view of the syscall counts map.
pub fn syscall_counts(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SYSCALL_BY_TYPE)?)?)
}

// syscall_bytes returns a typed view of the bytes moved, by process and syscall.
pub fn syscall_bytes(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SYSCALL_BYTES)?)?)
}

// socket_io returns a typed view of the reads and writes that reached a socket, by process and direction.
pub fn socket_io(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SOCKET_IO)?)?)
}

// connects returns a typed view of the connect attempts, by process and destination.
pub fn connects(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, ConnectKey, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::CONNECTS)?)?)
}

// runqueue_ns returns a typed view of the per-process run queue wait, in nanoseconds since load.
pub fn runqueue_ns(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u32, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::RUNQUEUE_NS)?)?)
}

// drops returns a typed view of the drops map.
pub fn drops(ebpf: &Ebpf) -> anyhow::Result<PerCpuArray<&MapData, u64>> {
    Ok(PerCpuArray::try_from(find(ebpf, map_names::DROPS)?)?)
}

// find_mut returns a writable reference to the map with the given name, or an error if it is not present.
fn find_mut<'a>(ebpf: &'a mut Ebpf, name: &str) -> anyhow::Result<&'a mut Map> {
    ebpf.map_mut(name)
        .with_context(|| format!("map {name} not in the eBPF object"))
}

// gpu_fds returns a typed view of the GPU FDS map.
pub fn gpu_fds(ebpf: &mut Ebpf) -> anyhow::Result<HashMap<&mut MapData, u64, u8>> {
    Ok(HashMap::try_from(find_mut(ebpf, map_names::GPU_FDS)?)?)
}

// gpu_activity_mut returns a writable view of the GPU activity map.
pub fn gpu_activity_mut(
    ebpf: &mut Ebpf,
) -> anyhow::Result<HashMap<&mut MapData, u32, GpuActivity>> {
    Ok(HashMap::try_from(find_mut(ebpf, map_names::GPU_ACTIVITY)?)?)
}
