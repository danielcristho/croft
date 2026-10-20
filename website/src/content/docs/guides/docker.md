---
title: Docker
description: Run the collector as a privileged container on one host.
sidebar:
  order: 1
---

## Build

```sh
cd croft
docker build -f deploy/docker/Dockerfile -t croft-collector .
```

The Dockerfile installs whichever Rust nightly is current and pins `LLVM_VERSION=23`. If a later nightly moves to another LLVM, the build fails at link time with `Invalid record`; bump the argument to match `rustc +nightly --version --verbose`.

## Run

```sh
sudo mkdir -p /var/lib/croft
docker run -d --name croft-collector --restart unless-stopped \
  --privileged --pid=host --cgroupns=host \
  -v /sys/kernel/tracing:/sys/kernel/tracing:ro \
  -v /var/lib/croft:/data \
  croft-collector

docker logs -f croft-collector
```

Each flag is needed:

| Flag | Why |
|---|---|
| `--privileged` | load the eBPF programs |
| `--pid=host` | see the host's processes, and backfill the ones already running |
| `--cgroupns=host` | read cgroup paths as on the host, not relative to the container |
| `-v /sys/kernel/tracing` | attach the tracepoints |
| `-v /var/lib/croft:/data` | keep the dataset after the container is gone |

`just docker-up` runs the same command and passes `WATCH_CGROUPS` from the environment. `croft deploy docker --dry-run` prints it.

## Check it works

Drive a GPU from another terminal:

```sh
python3 -c "import torch, time; x = torch.zeros(1).cuda(); time.sleep(30)"
```

Within a poll (2 s) the log shows a `gpu-onset` line and the dataset gains a row:

```text
22:31:07.412  INFO  reporter  workload=gpu-onset comm=<name> tgid=<pid> cpu=0.95 rss_mib=1412 ioctl=<n> syscalls=futex:282/s,...
```

Rows with no GPU ioctl in the last 5 s are `gpu-held` and logged at debug. If `nvidia-smi` reports a driver/library version mismatch, reboot first: CUDA does not open the device until the loaded driver matches the library.

A `refused ... inserts` warning means an eBPF map filled and samples were lost.

## Remove

```sh
docker rm -f croft-collector
```

`/var/lib/croft` keeps the dataset.
