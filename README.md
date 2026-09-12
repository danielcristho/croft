# Croft

Workload-aware Kubernetes autoscaling for AI workloads.

Traditional autoscalers react to CPU, memory, or request rate, after saturation.
Croft looks at what a workload is actually *doing* at the kernel level, through
eBPF, to decide when infrastructure should scale. It is a decision engine, not a
replacement for Karpenter or HPA.

> **Status: one signal collected, nothing classifies it yet.** GPU device
> activity is the first feature the eBPF program actually extracts. No workload
> classifier or scaling decision exists.

## Layout

```sh
collector/
├── collector/          userspace loader, attaches programs, reads maps
├── collector-common/   types shared across the kernel/userspace boundary
└── collector-ebpf/     the eBPF program, compiled to the BPF target
```

The eBPF program traces `openat`, `ioctl`, and `close` to detect GPU device
activity: a process that opens `/dev/nvidia*` or `/dev/dri/*` and then drives it
via `ioctl` gets a per-process activity counter, see [research.md's GPU
Gap](../.agent/croft/research.md) for why this is the first feature and how it
is derived. Syscall frequency, process lifetime, mmap counts, and futex counts
are still just proposed, not collected.

## Building

Recipes are in the [justfile](./justfile):

```bash
just build      # userspace + eBPF program
just run        # needs root to load and attach eBPF programs
just lint        # fmt --check and clippy
just test
```

### Toolchain

The eBPF crate needs a nightly toolchain and `bpf-linker`, which itself needs a
system LLVM matching nightly rustc's own LLVM version (`rustc +nightly --version
--verbose` shows it). bpf-linker 0.11 supports LLVM 21, 22, or 23; a mismatch
between nightly's LLVM and bpf-linker's fails at link time with `ERROR llvm:
Invalid record`, not at install time.

```bash
rustup toolchain install nightly --component rust-src
# install LLVM (apt.llvm.org's llvm.sh, or your distro's package) matching
# nightly rustc's LLVM version, then:
LLVM_SYS_231_PREFIX=/usr/lib/llvm-23 PATH=/usr/lib/llvm-23/bin:$PATH \
    cargo install bpf-linker --locked
```

`llvm-config` must be reachable unversioned (`llvm-config`, not
`llvm-config-23`) somewhere on `PATH`; a distro package typically only installs
the versioned name in `/usr/bin`, but the unversioned one lives alongside it
under `/usr/lib/llvm-<version>/bin`.

Userspace-only work does not need any of this.

## Where this is going

```sh
eBPF collector             GPU device activity collected
      │
      ▼
feature vectors             one feature so far
      │
      ▼
workload classifier        not built
      │
      ▼
decision engine             not built
      │
      ▼
Karpenter                   not built
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
