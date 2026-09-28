<div align="center">

# Croft

**Workload-aware autoscaling from what processes do in the kernel**

Watch every process that holds a GPU through eBPF, learn what kind of workload it
is, and tell the orchestrator before the pod, job or node request says so.

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Stars](https://img.shields.io/github/stars/Labs-CloudX/croft?style=flat)](https://github.com/Labs-CloudX/croft/stargazers)
[![PRs](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Labs-CloudX/croft/pulls)

[![Rust](https://img.shields.io/badge/Rust-2024-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![eBPF](https://img.shields.io/badge/eBPF-aya%200.14-FF6600)](https://aya-rs.dev)
[![Docker](https://img.shields.io/badge/Docker-privileged-2496ED?logo=docker&logoColor=white)](deploy/README.md)

[Deployment](deploy/README.md) · [CloudX](https://github.com/Labs-CloudX/cloudx)

</div>

---

> **Status: the collector records a labelled dataset; nothing decides yet.**
> Every GPU-holding process is written down every two seconds with its syscall
> rates and identity. The workload classifier and the decision engine are not built.

## What it does

A workload declares its size up front: a pod requests 2 CPU, a CloudX profile
asks for one GPU, a Nomad job reserves memory. What it then does can be
something else entirely, and every autoscaler that reads the declaration, or
waits for utilization to saturate, finds out late.

Croft reads the other side. It attaches to kernel tracepoints, notices when a
process starts driving a GPU, and records how it behaves: which syscalls, how
often, from which container. That record is the dataset a classifier learns
workload kinds from, and the classifier is what a decision engine will act on.

## How it works

| | |
|---|---|
| **Stable tracepoints only** | `openat`, `ioctl`, `close`, `raw_syscalls/sys_enter` and `sched_process_exit`. No kprobes on kernel internals, so the same program loads across kernel versions. |
| **A process is tracked from the moment it opens a GPU** | An `openat` on `/dev/nvidia*` or `/dev/dri/*` marks the fd; ioctls on it count as GPU activity. Processes that opened the device before croft started are found by scanning `/proc/<pid>/fd` once at startup. |
| **Every GPU holder is recorded, busy or quiet** | CUDA talks to the device with ioctls at start-up and then submits work through memory-mapped queues. A training job can run for hours without another ioctl, so croft writes a row for every holder on every poll and records how long since its last ioctl, instead of reading silence as idle. |
| **Rows carry identity, not only a pid** | `comm`, `exe`, the cgroup v2 path and the container id come from `/proc`, so a row can still be labelled after the process is gone. The command line is left out: its arguments can hold secrets. |
| **Lost updates are counted, not hidden** | A map insert the kernel refuses bumps a per-CPU drop counter; the collector warns with the map name and the count. |
| **Not tied to one orchestrator** | The collector knows kernels, processes and cgroups. Kubernetes, Docker, CloudX and Nomad are integrations at the end of the pipeline, not assumptions inside it. |

The GPU activity counter, from
[`collector-ebpf/src/gpu.rs`](collector/collector-ebpf/src/gpu.rs):

```rust
fn record_gpu_ioctl(ctx: &TracePointContext) -> Result<(), i32> {
    let fd: u64 = arg(ctx, FD_ARG)?;
    let tgid = current_tgid();
    // SAFETY: the reference is checked for presence and dropped on this line.
    if unsafe { GPU_FDS.get(pack_key(tgid, fd as u32)) }.is_none() {
        return Ok(());
    }
    // SAFETY: the helper takes no arguments and cannot fault.
    let now = unsafe { bpf_ktime_get_ns() };
    // SAFETY: a racing update loses one count, it does not corrupt state.
    let activity = GpuActivity::record(unsafe { GPU_ACTIVITY.get(tgid) }, now);
    count_on_err(
        GPU_ACTIVITY.insert(tgid, activity, 0),
        DropSite::GpuActivity,
    );
    Ok(())
}
```

Only ioctls on fds already known to be GPU devices are counted: one hash lookup
decides. The syscall counter on `raw_syscalls/sys_enter` is the expensive hook,
since it runs on every syscall of every process; its overhead is not measured yet.

## The dataset

One JSON line per GPU-holding process per poll, at `DATASET_PATH`:

```json
{
  "unix_ms": 1790598240036,
  "tgid": 1233064,
  "process": {
    "comm": "llama-server",
    "exe": "/usr/lib/ollama/llama-server",
    "cgroup": "/system.slice/docker-66c7e3ba6baf….scope",
    "container_id": "66c7e3ba6baf…"
  },
  "workload": "gpu-held",
  "ioctl_count": 452,
  "since_last_gpu_ioctl_ms": 6333,
  "syscalls": [{ "name": "clock_gettime", "rate_per_sec": 113.87 }, { "name": "poll", "rate_per_sec": 31.96 }],
  "label": null
}
```

`workload` is the collector's own placeholder, `gpu-onset` within 5 s of a GPU
ioctl and `gpu-held` after. `label` is left for training: controlled runs map
their container id to a known workload and join on it.

## Quick start

```sh
docker build -f deploy/docker/Dockerfile -t croft-collector .
sudo mkdir -p /var/lib/croft
just docker-up
docker logs -f croft-collector
```

The container needs `--privileged`, the host PID and cgroup namespaces, and the
host tracefs; `just docker-up` passes all of them. The
[deployment guide](deploy/README.md) covers each flag, a native build under
systemd, and how to check it works.

## Where this is going

<p align="center">
  <img src="assets/pipeline.svg" width="820" alt="Pipeline: eBPF collector, dataset, workload classifier, decision engine, then Kubernetes, Docker, CloudX or Nomad. The first two are built.">
</p>

Filled boxes are built; dashed ones are next.

Each integration acts through what its platform exposes. Karpenter, for one,
does not scale up on utilization: it provisions when the scheduler marks a pod
`Unschedulable=True`. So on Kubernetes croft shapes pending pods, for example low
priority placeholders that real work preempts, rather than reporting load. On
CloudX croft predicts which node should be warm and CloudX provisions it.

## Layout

| Path | What |
|---|---|
| `collector/collector-ebpf/` | The eBPF program: `gpu`, `syscalls`, `exit` tracepoints, `maps`, record `context` |
| `collector/collector-common/` | Types and helpers shared across the kernel/userspace boundary |
| `collector/collector/` | Userspace: loads and attaches, backfills, polls the maps, writes the dataset |
| `deploy/docker/` | Image that builds its own nightly, LLVM and `bpf-linker` |
| `deploy/systemd/` | Unit for a native install |

## Development

```sh
just build          # userspace and eBPF program
just build-release  # the same, optimized
just run            # debug build under sudo, in the foreground
just test           # cargo test
just lint           # fmt --check and clippy
just fmt            # apply formatting
just docker-up      # run the collector image with the flags it needs
```

### Toolchain

The eBPF crate needs a nightly toolchain and `bpf-linker`, which itself needs a
system LLVM matching nightly rustc's own LLVM version (`rustc +nightly --version
--verbose` shows it). bpf-linker 0.11 supports LLVM 21, 22, or 23; a mismatch
between nightly's LLVM and bpf-linker's fails at link time with `ERROR llvm:
Invalid record`, not at install time.

```sh
rustup toolchain install nightly --component rust-src
# install LLVM (apt.llvm.org's llvm.sh, or your distro's package) matching
# nightly rustc's LLVM version, then:
LLVM_SYS_231_PREFIX=/usr/lib/llvm-23 PATH=/usr/lib/llvm-23/bin:$PATH \
    cargo install bpf-linker --locked
```

`llvm-config` must be reachable unversioned (`llvm-config`, not
`llvm-config-23`) somewhere on `PATH`; a distro package installs the versioned
name in `/usr/bin`, and the unversioned one lives alongside it under
`/usr/lib/llvm-<version>/bin`.

Userspace-only work does not need any of this.

## License

Apache 2.0. See [LICENSE](LICENSE). The eBPF program is declared `Dual MIT/GPL`,
which the kernel requires for the helpers it calls.
