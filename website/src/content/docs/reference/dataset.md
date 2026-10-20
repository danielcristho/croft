---
title: Dataset
description: The row format, rotation into zstd segments, and reading it back.
sidebar:
  order: 3
---

One JSON line per process per 2 s poll, in `/var/lib/croft/croft-dataset.jsonl`.

```json
{
  "unix_ms": 1792505950473,
  "tgid": 234758,
  "process": {
    "comm": "python",
    "exe": "/usr/local/bin/python3.12",
    "cgroup": "/system.slice/docker-33813f5d….scope",
    "container_id": "33813f5d…",
    "cwd": "/workspace",
    "framework": "pytorch"
  },
  "workload": "gpu-held",
  "ioctl_count": 0,
  "since_last_gpu_ioctl_ms": 38794129,
  "usage": {
    "cpu_cores": 0.0, "rss_bytes": 1145397248, "threads": 104,
    "ctx_voluntary_s": 0.0, "ctx_involuntary_s": 0.0,
    "read_bytes_s": 0.0, "write_bytes_s": 0.0,
    "network_rx_bytes_s": 237.2, "network_tx_bytes_s": 514.4,
    "runqueue_s": 0.0
  },
  "cgroup_usage": {
    "cpu_cores": 0.004, "cpu_throttled_fraction": 0.0, "memory_bytes": 1511735296,
    "oom_kills": 0, "cpu_pressure_avg10": 0.0, "memory_pressure_avg10": 0.0, "io_pressure_avg10": 0.0
  },
  "syscalls": [{ "name": "futex", "rate_per_sec": 282.0 }]
}
```

| Field | Holds |
|---|---|
| `unix_ms`, `tgid` | time and process |
| `process` | `comm`, `exe`, `cgroup`, `container_id`, `cwd` (names the project of a host process), `framework` ([catalog](/croft/reference/frameworks/); left out when none) |
| `workload` | `gpu-onset`, `gpu-held` or `no-gpu` ([GPU states](/croft/reference/data-sources/#gpu-states)) |
| `ioctl_count`, `since_last_gpu_ioctl_ms` | GPU ioctls this poll, and time since the last one; left out without a GPU |
| `usage` | process CPU, RSS, threads, context switches, I/O and network bytes per second, `runqueue_s` |
| `cgroup_usage` | container `cpu_cores`, `cpu_throttled_fraction`, `memory_bytes`, `oom_kills`, PSI `*_pressure_avg10` |
| `syscalls` | `{name, rate_per_sec}` per syscall this poll |
| `label` | set by labelled experiments |

Idle processes are written once every 15 polls. Empty GPU fields are left out. Every row carries the container id, so experiments join their runs to the dataset by id.

## Rotation and retention

Every hour, or at 1 GiB, the collector renames the live file to `croft-dataset-<UTC start>.jsonl` and compresses it with zstd on a separate thread, about 13× smaller. It then deletes segments older than 30 days, and the oldest ones while all of them exceed 10 GiB.

```text
/var/lib/croft/
├── croft-dataset.jsonl                          live file
├── croft-dataset-20261020T130000Z.jsonl.zst     closed segments
└── croft-dataset-20261020T140000Z.jsonl.zst
```

[Configuration](/croft/reference/configuration/) changes the limits. A live file left by an older collector becomes a segment on the first poll.

## Reading it

- `croft stats` and `croft profile` read segments and plain files, and directories of them.
- On the shell: `zstd -dc croft-dataset-*.jsonl.zst | head`.
- In Python, with `zstandard`:

```python
import io, json, zstandard

with open("croft-dataset-20261020T130000Z.jsonl.zst", "rb") as f:
    reader = io.TextIOWrapper(zstandard.ZstdDecompressor().stream_reader(f))
    rows = [json.loads(line) for line in reader]
```

Filter large files with `grep -F -f ids.txt` on container ids before parsing JSON; parsing tens of gigabytes in Python takes many minutes.
