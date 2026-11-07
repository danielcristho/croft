---
title: Introduction
description: Why croft reads the kernel, and what it records.
sidebar:
  order: 1
---

The Kubernetes HPA scales on average CPU utilization, sampled every 15 s. When a burst arrives, the first replica is added half a minute later, and latency has already crossed its objective.

Croft looks earlier in the chain. It attaches to stable kernel tracepoints, reads `/proc` and the cgroup files beside them, and records, every two seconds and per container:

- the syscall rates of each process,
- its CPU, memory, threads, I/O and network,
- the throttling, pressure and OOM kills of its container,
- the time its threads waited for a CPU,
- the framework it runs,
- and whether it holds a GPU.

## Requests show up in the kernel first

A web server answers each request with about two `sendto` calls, so their rate is the request rate, seen from inside the node. Multiplied by the CPU each request costs, it gives the replicas the burst will need, within one poll of its start.

| Load | What happened |
|---|---|
| Step from 2 to 4 users | croft's request rate reacts in about 1 s, the pressure signal in 1–31 s, the HPA in 19–34 s |
| Open-loop burst, 5 → 75 RPS | croft's request rate reacts in 0–2 s; the HPA never scaled before the overloaded pod failed its probes |
| Same request on three nodes | it costs 1.69× more CPU on the slowest; the HPA averaged 67% while one replica throttled 34% |

[Results](/research/results/) has the full table and its caveats.

## Pipeline

```text
kernel tracepoints (eBPF)
       |
       v
collector ──> dataset (JSON Lines, one row per process per 2 s poll)
       |
       └────> /metrics (per container, latest poll)
                  |
                  v
          croft scale ──> Kubernetes Deployment replicas
```

Each stage stays replaceable: a stage never depends on one to its right.

## Where to go next

- [Quick start](/start/quick-start/) runs the collector on one machine with Docker.
- [Deploy on Kubernetes](/guides/kubernetes/) runs one collector per node.
- [Data sources](/reference/data-sources/) lists what comes from eBPF and what from files.
