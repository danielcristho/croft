# Project

## Overview

croft reads what processes do in the kernel through eBPF (syscall rates, CPU,
pressure, throttling, GPU use) and uses those signals to scale Kubernetes
Deployments. It is a Cloud Computing course final project that compares
eBPF-based autoscaling with the HPA on Kubernetes microservices.

- Rust (edition 2024) for the collector, the eBPF program and the `croft` CLI.
- Python 3.12+ and `uv` for the experiment harness.
- Jupyter notebooks for the analysis, in the gitignored `research/`.
- `just` as the task runner.
- k3s for the lab cluster, set up with Ansible.
- Astro for the documentation site in `website/`.

LLM inference autoscaling is croft's next research, not this project.

## Repository Layout

```
agent/                         working notes for coding agents
collector/collector/src/       userspace collector and CLI
  cli.rs                       arguments of every command
  command/                     one file per command; scale.rs is croft scale
  decision/scaler.rs           syscall rate to replica count
  pipeline/, sensing/          metrics, dataset and eBPF map readers
collector/collector-ebpf/src/  eBPF programs: syscalls.rs, gpu.rs, runqueue.rs
collector/collector-common/    types shared between userspace and eBPF
deploy/                        docker, kubernetes and systemd deployment
labs/k3s-cluster/              Ansible setup of the lab cluster
labs/research/experiment/      experiment harness
  apps.json                    applications under test and their calibration
  experiment/                  kube, prometheus, records, rps, profiles, web, compare
  workloads/                   application and monitoring manifests
  assets/                      diagrams and charts for reports
  data/                        run output, gitignored
research/                      notebooks and local analysis, gitignored
docs/                          local design notes, gitignored
website/                       documentation site
```

## Commands of the CLI

```sh
croft run       # the collector; needs root
croft top       # processes of the newest poll
croft stats     # dataset summary
croft profile   # resource profile of a container over a window, as JSON
croft deploy    # start the collector on Docker or Kubernetes
croft collect   # pull each node's dataset from the DaemonSet
croft scale     # scale one Deployment from its calibrated syscall rate
```

The default dataset is `/var/lib/croft/croft-dataset.jsonl`. Older rows sit in
zstd segments `croft-dataset-<UTC start>.jsonl.zst` beside it.

## Applications Under Test

| App | Namespace | Scaled Deployment | Calibrated syscall |
|---|---|---|---|
| hotel-reservation | `hotel-res` | `frontend` | `write` |
| online-boutique | `boutique` | `frontend` | `getpeername` |
| otel-demo | `otel-demo` | `frontend` | `writev` |

Calibration lives in `apps.json`. It is measured per application and only for
`frontend`. Boutique's `getpeername` likely counts DNS lookups (#39), so recheck
it before relying on it under a new endpoint mix.

## Current Plan

Open issues #37 to #47 hold the plan: per-service analysis, syscall patterns,
syscall operands, and a final series of the HPA against croft alone. Read them
before starting experiment work.

## Environment Variables

```
CROFT_METRICS_ADDR    address of the collector's /metrics; croft scale needs it
CROFT_ROTATE_BYTES    dataset rotation by size
CROFT_ROTATE_SECS     dataset rotation by age
CROFT_RETAIN_BYTES    dataset retention by size
CROFT_RETAIN_DAYS     dataset retention by age
KUBECONFIG            ~/.kube/croft-k3s.yaml for the lab cluster
JUPYTER_TOKEN         JupyterLab token, in research/.env
```

## Development Commands

Prefer the recipes in `justfile`:

```sh
just build                      # collector and eBPF program
just test                       # cargo test
just lint                       # fmt --check and clippy -D warnings
just fmt                        # apply formatting
just samples list               # recipes of the experiment harness
just samples burst <app> <arm>  # one burst run; arm is none or croft
just samples calibrate <app>    # measure an application's calibration
just samples compare <prefix>   # score finished runs
```

The eBPF crate needs nightly Rust and `bpf-linker`; `deploy/docker/Dockerfile`
shows the setup. Userspace-only work does not. Enable the hooks once per clone:

```sh
git config core.hooksPath .githooks
```
