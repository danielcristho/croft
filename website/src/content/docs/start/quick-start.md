---
title: Quick start
description: Run the collector on one Linux machine with Docker.
sidebar:
  order: 2
---

The collector runs on each machine it observes, as root, on Linux only. Docker needs no toolchain on the host: the image builds its own Rust nightly, LLVM and `bpf-linker`, so the first build takes a while.

## Requirements

- Linux with eBPF and tracefs, which any recent distribution kernel has. `mount | grep tracefs` shows `/sys/kernel/tracing`.
- cgroup v2.
- Root, or a privileged container.

## Build and run

```sh
git clone https://github.com/danielcristho/croft.git
cd croft
docker build -f deploy/docker/Dockerfile -t croft-collector .

sudo mkdir -p /var/lib/croft
just docker-up
```

`just docker-up` runs the image with the flags it needs. Without `just`, see [Docker](/croft/guides/docker/).

## Look at what it records

```sh
docker exec -it croft-collector croft top      # processes of the newest poll
docker exec -it croft-collector croft stats    # dataset summary
```

By default only processes holding a GPU get rows. To record every process of some containers, GPU or not, set `WATCH_CGROUPS`:

```sh
WATCH_CGROUPS=docker-<container-id> just docker-up
```

The dataset is in `/var/lib/croft/croft-dataset.jsonl`; [Dataset](/croft/reference/dataset/) describes a row.
