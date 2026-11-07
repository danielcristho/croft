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
| `CROFT_FRAMEWORKS` | built in | path to a [framework catalog](/reference/frameworks/) that replaces the built-in one |
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
| `croft_container_syscall_bytes_total` | `container_id`, `syscall` |
| `croft_container_connects_total` | `container_id`, `address`, `port` |
| `croft_container_socket_io_total` | `container_id`, `direction` |

The `_total` metrics are counters of what syscalls carry, so read them with `rate()`:

- `syscall_bytes` sums the return values of the read and write family, on x86_64.
- `connects` counts attempts to IPv4 and IPv6 destinations at entry, so it includes attempts that later fail.
- `socket_io` counts the read and write family calls that reached a socket. The collector warns and leaves it empty when the kernel lacks `sock_read_iter` or `sock_write_iter`.

A counter drops when one process of a container exits, and `rate()` reads that as a reset.

The probes read no syscall buffer. `connect`'s address is the only user memory they read for these metrics.

`croft scale` reads this endpoint from every collector pod.
