<div align="center">

# Croft

**Autoscaling that reacts to what processes do in the kernel**

Read each pod's request rate through eBPF and its CPU pressure from cgroups,
notice a workload burst within seconds, and add replicas earlier than a
CPU-based autoscaler.

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Stars](https://img.shields.io/github/stars/danielcristho/croft?style=flat)](https://github.com/danielcristho/croft/stargazers)
[![PRs](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/danielcristho/croft/pulls)

[![Rust](https://img.shields.io/badge/Rust-2024-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![eBPF](https://img.shields.io/badge/eBPF-aya%200.14-FF6600)](https://aya-rs.dev)

[Deployment](deploy/README.md)

</div>

---

> **Status: the collector records a dataset; nothing scales yet.**
> Early burst detection is measured offline against the HPA; the detector and
> the scaler are not built.

## What it does

The Kubernetes HPA scales on average CPU utilization, sampled every 15 s. When a
burst arrives, the first replica is added half a minute later, and latency has
already crossed its objective. Croft looks earlier in the chain: it attaches to
stable kernel tracepoints, reads `/proc` and the cgroup files beside them, and
records, every two seconds and per container, the syscall rates, CPU, memory,
throttling and pressure of each process, the framework it runs, and which
processes drive a GPU.

Requests show up in the kernel before they show up as CPU. A web server answers
each request with about two `sendto` calls, so their rate is the request rate,
seen from inside the node. Multiplied by the CPU each request costs, it gives the
replicas the burst will need, within one poll of its start.

<p align="center">
  <img src="assets/architecture.svg" width="860" alt="Architecture: the kernel reports a web app's syscalls through eBPF to a collector on each node. The collector writes a dataset and serves metrics to Prometheus and to croft scale, which sets the number of pods.">
</p>

## What it reads

eBPF sees events, which no file keeps: each syscall, each wait for a CPU, each
call into the GPU driver. Counters that the kernel already keeps are read from
files, at no cost to the traced processes.

| Source | Gives | Fields |
|---|---|---|
| eBPF, `raw_syscalls/sys_enter` | syscall rates; the request rate of a web server | `syscalls` |
| eBPF, `sched_wakeup` / `sched_switch` | time each process waited for a CPU | `runqueue_s` |
| eBPF, `openat`, `ioctl`, `close` on `/dev/nvidia*` | which processes hold a GPU and when they start using it; the only host processes recorded | `workload` |
| `/proc/<pid>/maps`, `cmdline` | framework, from the libraries the process loaded, or its arguments when it loads none of its own ([`frameworks.json`](collector/collector/frameworks.json)) | `framework` |
| `/proc/<pid>/{stat,status,io,cwd,cgroup}`, `net/dev` | CPU, RSS, threads, context switches, disk and network bytes, working directory, container | `usage`, `process` |
| cgroup v2 `cpu.stat`, `memory.current`, `*.pressure`, `memory.events` | throttling, container memory, PSI, OOM kills | `cgroup_usage` |

The row format is in [`agent/signals.md`](agent/signals.md).

## Quick start

```sh
docker build -f deploy/docker/Dockerfile -t croft-collector .
sudo mkdir -p /var/lib/croft
just docker-up
docker exec -it croft-collector croft top
```

The [deployment guide](deploy/README.md) covers Kubernetes, systemd and each
flag the container needs.

## Commands

```sh
croft run       # the collector; needs root
croft top       # processes of the newest poll
croft stats     # dataset summary
croft deploy    # start the collector on Docker or Kubernetes
croft collect   # pull each node's dataset from the DaemonSet
```

`croft <command> --help` lists the options.

## Experiments

Burst, ramp and step experiments on a k3s cluster, with the HPA as the baseline,
live in [`labs/research/experiment`](labs/research/experiment/README.md); the
analysis notebooks are in [`research`](research).

```sh
cd labs/research/experiment
uv run run.py web --prefix burst-1 --profile burst --scale 0.5
```

## Development

```sh
just build    # userspace and eBPF program
just test     # cargo test
just lint     # fmt --check and clippy
just fmt      # apply formatting
```

The eBPF crate needs nightly Rust and `bpf-linker` built against a matching
LLVM; the [Dockerfile](deploy/docker/Dockerfile) shows the setup.
Userspace-only work does not need it.

Enable the commit hooks once per clone: `git config core.hooksPath .githooks`.

## License

Apache 2.0. See [LICENSE](LICENSE). The eBPF program is declared `Dual MIT/GPL`,
which the kernel requires for the helpers it calls.

Project documents: [maintainers](MAINTAINERS.md), [governance](GOVERNANCE.md),
[roadmap](ROADMAP.md), [security](SECURITY.md).
