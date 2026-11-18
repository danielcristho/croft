<div align="center">

# Croft

**Kubernetes autoscaling that reacts to eBPF kernel signals**

[![CI](https://github.com/danielcristho/croft/actions/workflows/ci.yml/badge.svg)](https://github.com/danielcristho/croft/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Artifact Hub](https://img.shields.io/endpoint?url=https://artifacthub.io/badge/repository/croft)](https://artifacthub.io/packages/search?repo=croft)

[Docs](https://croft-docs.pages.dev) · [Getting started](#getting-started) · [Deployment](deploy/README.md) · [Experiments](labs/research/experiment/README.md) · [Roadmap](ROADMAP.md) · [Contributing](CONTRIBUTING.md)

</div>

Croft is a Kubernetes autoscaler for web services, built on eBPF and written in
Rust with [Aya](https://aya-rs.dev). It watches what each pod asks of the kernel, turns that into a request rate, and adds replicas
without waiting for average CPU to climb.

The Horizontal Pod Autoscaler (HPA) scales on CPU utilization, which rises after
requests arrive and reaches the HPA through a metrics pipeline. Requests show up
in the kernel first. A web server makes a steady number of syscalls per request,
so the rate of one syscall gives the request rate, seen from inside the node.
Multiplied by the CPU each request costs, it gives the replicas the load needs.

<p align="center">
  <img src="assets/scaling-flow.svg" width="760" alt="A burst of requests reaches a pod. The HPA reads its CPU every 15 seconds; croft reads its syscalls through eBPF every 2 seconds. Both set the number of replicas.">
</p>

Croft has two parts: a collector, which runs on every node and records what each
process does, and `croft scale`, which sets the replica count of one Deployment
from the collector's signals.

<p align="center">
  <img src="assets/architecture.svg" width="860" alt="croft architecture: inside the Linux kernel, eBPF programs and kernel files record syscall rates, bytes moved, CPU wait, connections, socket I/O, GPU use, CPU pressure and processes for your app's pods. The croft collector reads them every 2 seconds and writes a dataset, serves Prometheus and feeds croft scale, which sets the number of pods.">
</p>

## What the lab shows so far

Measured on a three-node k3s cluster against the HPA at a 70% CPU target
([full results](https://croft-docs.pages.dev/research/results/)):

| Load | croft's request rate reacts in | The HPA reacts in |
|---|---|---|
| Step from 2 to 4 users | about 1 s | 19–34 s |
| Open-loop burst, 5 → 75 RPS, default probes | 0–2 s | never: the overloaded pod failed its probes and the metrics API returned nothing |

> [!IMPORTANT]
> Croft is experimental and runs in a lab cluster, not in production. It reacts
> earlier than the HPA; whether that keeps latency within the SLO is still being
> measured. See [Limitations](#limitations).

## How it compares

| | HPA | KEDA | croft |
|---|---|---|---|
| Signal | CPU or memory use from metrics-server; other metrics through an adapter | an external metric: a queue, Prometheus, a cloud service | syscall rate per container, from eBPF |
| Read every | 15 s | 30 s by default | 2 s |
| Needs from the app | a CPU request | an exporter or a queue to read | a calibration: syscalls and CPU per request |
| Scale to zero | no | yes | no |
| Runs | in the control plane | as an operator, on top of the HPA | as a DaemonSet with root, plus one `croft scale` per Deployment |

## Features

### Per-container signals from the kernel

The collector attaches to kernel tracepoints and reads `/proc` and cgroup files
beside them. Every two seconds it records each container's syscall rates, CPU,
memory, throttling, pressure and run queue wait. It needs no change to the
application, no sidecar and no instrumentation library.

### Scaling on the request rate

`croft scale` reads the syscall calibrated for an application from the collector
on each node, and asks for more replicas after two polls in a row agree. It uses
the same target and the same upper bound as the HPA it replaces, and waits 300 s
before scaling down.

### A dataset for offline analysis

Each poll appends a row per process to a JSON Lines dataset, rotated and
compressed into zstd segments. `croft stats`, `croft top` and `croft profile` read
it, and `croft collect` pulls it from every node of a cluster.

### GPU workload detection

Calls to `/dev/nvidia*` show which processes hold a GPU and when they start using
it. The libraries a process loads show its framework, from a
[catalog you can extend](collector/collector/frameworks.json).

### Limited reads

The eBPF programs read user memory in two places only: the path given to `openat`,
to spot `/dev/nvidia*`, and the address given to `connect`. They do not read
syscall buffers or environment variables.

## What it reads

eBPF sees events, which no file keeps. Counters that the kernel already keeps are
read from files, at no cost to the traced processes.

| Source | Gives | Published as |
|---|---|---|
| `raw_syscalls/sys_enter` | syscall rates; the request rate of a web server | `syscalls`, `croft_container_syscall_rate` |
| `raw_syscalls/sys_exit` | bytes moved by read and write syscalls | `croft_container_syscall_bytes_total` |
| `sched_wakeup`, `sched_switch` | time each process waited for a CPU | `runqueue_s`, `croft_container_runqueue_wait` |
| `sys_enter_connect` | connect attempts by destination | `croft_container_connects_total` |
| `sock_read_iter`, `sock_write_iter` | reads and writes that reached a socket | `croft_container_socket_io_total` |
| `openat`, `ioctl`, `close` on `/dev/nvidia*` | which processes hold a GPU and when they start using it | `workload` |
| `/proc/<pid>/maps`, `cmdline` | framework, from loaded libraries or arguments | `framework` |
| `/proc/<pid>/{stat,status,io,cwd,cgroup}`, `net/dev` | CPU, RSS, threads, context switches, disk and network bytes, container | `usage`, `process` |
| cgroup v2 `cpu.stat`, `memory.current`, `*.pressure`, `memory.events` | throttling, container memory, PSI, OOM kills | `cgroup_usage`, `croft_container_cpu_*` |

The dataset's row format is in [`agent/signals.md`](agent/signals.md).

## Getting started

### Kubernetes

The Helm chart runs the collector on every node:

```sh
helm install croft oci://ghcr.io/danielcristho/charts/croft -n croft --create-namespace
kubectl -n croft get pods -o wide
```

Each release publishes the chart and the image with the same version; add
`--version X.Y.Z` to pin one. From a clone, `deploy/helm/croft` installs the
chart of `main`.

To scale a Deployment, add it under `scaler.targets` with `dryRun: true`, watch
the decisions in the log, then turn `dryRun` off and remove the Deployment's HPA.
The [deployment guide](deploy/README.md#111-scale-your-own-deployment) shows the
values.

### The CLI

`croft` runs `scale`, `collect`, `top` and the other commands from your own
machine. Each release ships it for Linux x86-64:

```sh
curl -LO https://github.com/danielcristho/croft/releases/latest/download/croft-linux-amd64
curl -LO https://github.com/danielcristho/croft/releases/latest/download/croft-linux-amd64.sha256
echo "$(cat croft-linux-amd64.sha256)  croft-linux-amd64" | sha256sum --check
sudo install -m 0755 croft-linux-amd64 /usr/local/bin/croft
croft --version
```

Replace `latest/download` with `download/vX.Y.Z` for one release. Without root,
install it to `~/.local/bin` instead. The binary needs glibc 2.34 or newer
(Ubuntu 22.04, Debian 12, RHEL 9); on an older host, use the Docker image.

### One host with Docker

Build the image and start the collector on one host:

```sh
docker build -f deploy/docker/Dockerfile -t croft-collector .
sudo mkdir -p /var/lib/croft
just docker-up
docker exec -it croft-collector croft top
```

The [deployment guide](deploy/README.md) covers Kubernetes, systemd and each flag
the container needs.

### Scale a Deployment

`croft scale` needs two numbers measured for the application: the calls of one
syscall per request, and the CPU one request costs.

```sh
croft scale \
  --namespace hotel-res \
  --deployment frontend \
  --syscall write \
  --calls-per-request 11.4 \
  --cpu-per-request-ms 1.6
```

[`deploy/kubernetes/scaler.yaml`](deploy/kubernetes/scaler.yaml) runs it in a
cluster. [`RUNNING.md`](labs/research/experiment/RUNNING.md) shows how to measure
the two numbers.

## Limitations

- **Calibration per application.** `croft scale` needs the syscalls and the CPU
  one request costs, measured for each application.
- **It counts answered requests.** `sendto` rises with requests a pod answers, so
  a saturated pod shows its capacity, not the offered load. The queue of
  connections waiting to be accepted would show it (issue #11).
- **Root on every node.** The collector loads eBPF programs, so it runs as root or
  in a privileged container. [Limited reads](#limited-reads) lists what it reads.
- **One container per pod.** `croft scale` sizes replicas from the CPU request of
  the pod's first container.
- **No scale to zero.** It keeps at least one replica.

## Commands

| Command | Does |
|---|---|
| `croft run` | runs the collector on this host; needs root |
| `croft top` | shows the processes of the newest poll |
| `croft stats` | summarizes datasets |
| `croft profile` | prints a container's resource profile over a window, as JSON |
| `croft deploy` | starts the collector on Docker or Kubernetes |
| `croft collect` | pulls each node's dataset from the DaemonSet |
| `croft scale` | sets a Deployment's replicas from the request rate the collectors see |

`croft <command> --help` lists the options.

## Experiments

A harness on a k3s cluster runs [DeathStarBench Hotel Reservation](https://github.com/delimitrou/DeathStarBench),
[Online Boutique](https://github.com/GoogleCloudPlatform/microservices-demo) and the
[OpenTelemetry Demo](https://github.com/open-telemetry/opentelemetry-demo) under
open-loop `burst` and `ramp` load.

- [`RUNNING.md`](labs/research/experiment/RUNNING.md): the procedure, step by step
- [`SETUP.md`](labs/research/experiment/SETUP.md): the load, autoscaling and cluster configuration

```sh
cd labs/research/experiment
uv run run.py web --app hotel-reservation --prefix burst-1 --profile burst --probes relaxed
```

## Development

```sh
just build    # userspace and eBPF program
just test     # cargo test
just lint     # fmt --check and clippy
just fmt      # apply formatting
```

The collector is Rust on both sides of the kernel boundary: the eBPF programs use
`aya-ebpf`, and userspace loads them and reads their maps with `aya`. No C or
libbpf is involved.

The eBPF crate needs nightly Rust and `bpf-linker` built against a matching LLVM;
the [Dockerfile](deploy/docker/Dockerfile) shows the setup. Userspace-only work
does not need it.

Enable the commit hooks once per clone: `git config core.hooksPath .githooks`.

## Community

- [Contributing](CONTRIBUTING.md)
- [Code of conduct](CODE_OF_CONDUCT.md)
- [Governance](GOVERNANCE.md) and [maintainers](MAINTAINERS.md)
- [Security policy](SECURITY.md)
- [Roadmap](ROADMAP.md)

## License

Apache 2.0. See [LICENSE](LICENSE). The eBPF program is declared `Dual MIT/GPL`,
which the kernel requires for the helpers it calls.

The collector workspace started from [aya-template](https://github.com/aya-rs/aya-template)
(MIT); [collector/README.md](collector/README.md) keeps its notice.
