//! Typed access to the eBPF maps by their shared names.

use anyhow::Context as _;
use aya::{
    Ebpf,
    maps::{HashMap, Map, MapData, PerCpuArray},
};
use collector_common::{ConnectKey, GpuActivity, map_names};

fn find<'a>(ebpf: &'a Ebpf, name: &str) -> anyhow::Result<&'a Map> {
    ebpf.map(name)
        .with_context(|| format!("map {name} not in the eBPF object"))
}

pub fn gpu_activity(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u32, GpuActivity>> {
    Ok(HashMap::try_from(find(ebpf, map_names::GPU_ACTIVITY)?)?)
}

pub fn syscall_counts(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SYSCALL_BY_TYPE)?)?)
}

pub fn syscall_bytes(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SYSCALL_BYTES)?)?)
}

pub fn socket_io(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SOCKET_IO)?)?)
}

pub fn server_fds(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u64, u8>> {
    Ok(HashMap::try_from(find(ebpf, map_names::SERVER_FDS)?)?)
}

pub fn connects(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, ConnectKey, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::CONNECTS)?)?)
}

pub fn runqueue_ns(ebpf: &Ebpf) -> anyhow::Result<HashMap<&MapData, u32, u64>> {
    Ok(HashMap::try_from(find(ebpf, map_names::RUNQUEUE_NS)?)?)
}

pub fn drops(ebpf: &Ebpf) -> anyhow::Result<PerCpuArray<&MapData, u64>> {
    Ok(PerCpuArray::try_from(find(ebpf, map_names::DROPS)?)?)
}

fn find_mut<'a>(ebpf: &'a mut Ebpf, name: &str) -> anyhow::Result<&'a mut Map> {
    ebpf.map_mut(name)
        .with_context(|| format!("map {name} not in the eBPF object"))
}

pub fn gpu_fds(ebpf: &mut Ebpf) -> anyhow::Result<HashMap<&mut MapData, u64, u8>> {
    Ok(HashMap::try_from(find_mut(ebpf, map_names::GPU_FDS)?)?)
}

pub fn gpu_activity_mut(
    ebpf: &mut Ebpf,
) -> anyhow::Result<HashMap<&mut MapData, u32, GpuActivity>> {
    Ok(HashMap::try_from(find_mut(ebpf, map_names::GPU_ACTIVITY)?)?)
}
