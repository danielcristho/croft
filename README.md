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

[Deployment](deploy/README.md) · [CloudX](https://github.com/Labs-CloudX/cloudx)

</div>

---

> **Status: the collector records a labelled dataset; nothing decides yet.**
> The workload classifier and the decision engine are not built.

## What it does

A workload declares its size up front, and what it then does can be something
else entirely. Croft reads the other side: it attaches to stable kernel
tracepoints, notices when a process starts driving a GPU, and records every two
seconds its syscall rates, CPU, memory, throttling and pressure, with the
container it runs in. That dataset is what a classifier learns workload kinds
from, and what a decision engine will act on, on Kubernetes, Docker Swarm, Nomad
or CloudX.

<p align="center">
  <img src="assets/pipeline.svg" width="820" alt="Pipeline: eBPF collector, dataset, workload classifier, decision engine, then Kubernetes, Docker, CloudX or Nomad. The first two are built.">
</p>

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
