---
title: Data sources
description: What comes from eBPF, and what from /proc and cgroup files.
sidebar:
  order: 2
---

eBPF sees events, which no file keeps: each syscall, each wait for a CPU, each call into the GPU driver. Counters the kernel already keeps are read from files, at no cost to the traced processes.

| Source | Gives | Fields |
|---|---|---|
| eBPF, `raw_syscalls/sys_enter` | syscall rates; the request rate of a web server | `syscalls` |
| eBPF, `sched_wakeup`, `sched_wakeup_new`, `sched_switch` | time each process waited for a CPU | `usage.runqueue_s` |
| eBPF, `openat`, `ioctl`, `close` on `/dev/nvidia*` and `/dev/dri/*` | which processes hold a GPU and when they call into it | `workload`, `ioctl_count`, `since_last_gpu_ioctl_ms` |
| eBPF, `raw_syscalls/sys_exit` of `accept`, reads and writes, `close` | accepted connections holding a request not yet answered | `croft_container_inflight_requests` on `/metrics` |
| eBPF, `sched_process_exit` | clears a process's state when it exits | |
| `/proc/<pid>/maps`, `cmdline` | framework, from the libraries a process loaded or its arguments | `process.framework` |
| `/proc/<pid>/stat`, `status`, `io`, `cwd`, `cgroup`, `net/dev` | CPU, RSS, threads, context switches, disk and network bytes, working directory, container | `usage`, `process` |
| cgroup v2 `cpu.stat`, `memory.current`, `*.pressure`, `memory.events` | container CPU and throttling, memory, PSI, OOM kills | `cgroup_usage` |

## Which processes get rows

- Every process holding a GPU device. On the host, these are the only processes recorded.
- With `WATCH_CGROUPS`, every process in a cgroup whose path contains one of its substrings, GPU or not: `kubepods` for every pod, `docker` for every container. These come from walking `/sys/fs/cgroup`, so a CPU-bound loop that makes no syscalls is still recorded.

## GPU states

| `workload` | Means |
|---|---|
| `gpu-onset` | holds a GPU fd and sent a GPU ioctl in the last 5 s: setting up, allocating, launching |
| `gpu-held` | holds a GPU fd with no ioctl in the last 5 s, often busy on mapped queues; also a process that opened the GPU before the collector started |
| `no-gpu` | holds no GPU fd |

Holding a GPU is not using it: utilization needs NVML (issue #13).

## Request rate from syscalls

| Signal | Use |
|---|---|
| `sendto` rate ÷ 2.06 | request rate as the kernel sees it for the lab webapp; correlation about 0.8 with the load generator |
| `recvfrom`, `epoll_wait` | also about one per request |
| `accept4` | not usable: ingress-nginx keeps upstream connections alive |
| `network_rx_bytes_s` | tracks requests; counted per network namespace, so take the max per pod |
| `cpu_pressure_avg10 > 10` or `cpu_throttled_fraction > 0.2`, two polls | pressure rule |
| `runqueue_s` | seconds per second the threads waited for a CPU, summed over threads; reacts within a 2 s window where PSI averages 10 s |

`sendto` counts answered requests. Once a pod saturates, it shows the pod's capacity, not the offered load (issue #11).

## In-flight requests

The collector marks each connection a process accepts. A read that returns data marks it as holding a request; a write marks it answered; `close` forgets it. `croft_container_inflight_requests` counts the connections holding a request, summed per container: the same quantity an app exports as an in-flight gauge, read without changing the app.

| Limit | Effect |
|---|---|
| Connections accepted before the collector started | not counted until they reconnect; ingress-nginx keeps upstream connections open for a long time |
| HTTP/2 and gRPC | many requests share one connection, which counts once |
| A queue inside the app's memory | leaves no trace in a socket; the kernel cannot see it |
| Connections the process opened itself | never counted, so a call to a database or another service is not an in-flight request |
