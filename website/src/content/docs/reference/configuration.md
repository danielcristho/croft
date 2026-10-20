---
title: Configuration
description: Environment variables of the collector, and its /metrics endpoint.
sidebar:
  order: 5
---

Every option of `croft run` has an environment variable, so the same image is configured in Docker, Kubernetes and systemd alike.

| Variable | Default | Effect |
|---|---|---|
| `DATASET_PATH` | `/var/lib/croft/croft-dataset.jsonl` | live dataset file; segments are written beside it |
| `WATCH_CGROUPS` | empty | comma-separated substrings; every process in a matching cgroup gets rows, GPU or not |
| `CROFT_FRAMEWORKS` | built in | path to a [framework catalog](/croft/reference/frameworks/) that replaces the built-in one |
| `CROFT_METRICS_ADDR` | empty | serve the latest poll at `http://ADDR/metrics`, e.g. `0.0.0.0:9464`; off when empty |
| `CROFT_ROTATE_SECS` | `3600` | close the live file into a segment after this long |
| `CROFT_ROTATE_BYTES` | `1073741824` | or once it reaches this size |
| `CROFT_RETAIN_DAYS` | `30` | delete segments older than this |
| `CROFT_RETAIN_BYTES` | `10737418240` | delete the oldest segments while all of them exceed this |
| `RUST_LOG` | `info` | log level; `debug` also logs `gpu-held` rows |

`0` turns a rotation or retention limit off.

## /metrics

With `CROFT_METRICS_ADDR` set, the collector serves the latest poll in the Prometheus text format, summed per container:

| Metric | Labels |
|---|---|
| `croft_poll_unix_ms` | |
| `croft_container_cpu_cores` | `container_id` |
| `croft_container_cpu_pressure_avg10` | `container_id` |
| `croft_container_cpu_throttled_fraction` | `container_id` |
| `croft_container_runqueue_wait` | `container_id` |
| `croft_container_syscall_rate` | `container_id`, `syscall` |

`croft scale` reads it from every collector pod.
