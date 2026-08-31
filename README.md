# Croft

Workload-aware Kubernetes autoscaling for AI workloads.

Traditional autoscalers react to CPU, memory, or request rate, after saturation.
Croft looks at what a workload is actually *doing* at the kernel level, through
eBPF, to decide when infrastructure should scale. It is a decision engine, not a
replacement for Karpenter or HPA.

> **Status: scaffold.** The collector crate is generated but does not build yet
> (see [Blocked on bpf-linker](#blocked-on-bpf-linker)). Nothing classifies
> workloads or makes scaling decisions.

## Layout

```sh
collector/
├── collector/          userspace loader and event reader
├── collector-common/   types shared across the boundary
└── collector-ebpf/     the eBPF program, compiled to the BPF target
```

The eBPF program attaches to the `raw_syscalls/sys_enter` tracepoint. Syscall
frequency, process lifetime, mmap counts, and futex counts all derive from it.
Process events such as exec and exit come later, as a second program in the same
crate.

## Building

```bash
cd collector
cargo build              # userspace only
cargo build --release
```

The eBPF crate needs a nightly toolchain and `bpf-linker`:

```bash
rustup toolchain install nightly --component rust-src
```

### Blocked on bpf-linker

`cargo install bpf-linker` does not work on every machine, and the crate says so
itself:

> Installing bpf-linker through `cargo install` is NOT recommended for regular
> users due to dependency on specific LLVM version, system libraries and overall
> complexity of getting the setup right.

bpf-linker 0.11 supports LLVM 21, 22, or 23 only. Older system LLVM will not do.
Check what is installed:

```bash
ls /usr/lib/llvm-*/bin/llvm-config
LLVM_PREFIX=/usr/lib/llvm-21 cargo install bpf-linker
```

Also make sure `PATH` holds no directory the current user cannot read. The build
script walks `PATH` looking for `llvm-config` and fails on the first unreadable
entry, which produces a confusing "Permission denied" rather than a missing
LLVM error.

Userspace work does not need any of this.

## Where this is going

```sh
eBPF collector
      │
      ▼
feature vectors
      │
      ▼
workload classifier        not built
      │
      ▼
decision engine            not built
      │
      ▼
Karpenter                  not built
```

One note on the last step. Karpenter does not scale up on utilization. It
provisions when the scheduler marks pods `Unschedulable=True`, and uses
utilization only for consolidation. So croft cannot report load and expect a
node. The lever is to shape pending pods, for example low priority placeholder
pods that real work then preempts.

Karpenter now runs on bare metal and private cloud through vCluster Auto Nodes,
so this path is open outside the public cloud.

## License

[Apache 2.0](./LICENSE)
