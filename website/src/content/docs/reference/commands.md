---
title: Commands
description: Every croft subcommand and its options.
sidebar:
  order: 1
---

```text
croft run       the collector; needs root
croft top       processes of the newest poll
croft stats     dataset summary
croft profile   resource profile of one container, as JSON
croft deploy    start the collector on Docker or Kubernetes
croft collect   pull each node's dataset from the DaemonSet
croft scale     add replicas from the request rate, ahead of the HPA
```

`croft <command> --help` lists the options. Paths default to `/var/lib/croft/croft-dataset.jsonl`, or `DATASET_PATH`.

## run

Loads the eBPF programs, polls every 2 s and writes the dataset. Needs root.

| Option | Env | Default |
|---|---|---|
| `--dataset` | `DATASET_PATH` | `/var/lib/croft/croft-dataset.jsonl` |
| `--watch` | `WATCH_CGROUPS` | empty: only GPU holders |
| `--frameworks` | `CROFT_FRAMEWORKS` | the built-in catalog |
| `--metrics-addr` | `CROFT_METRICS_ADDR` | empty: no `/metrics` |
| `--rotate-secs` | `CROFT_ROTATE_SECS` | `3600` |
| `--rotate-bytes` | `CROFT_ROTATE_BYTES` | `1073741824` (1 GiB) |
| `--retain-days` | `CROFT_RETAIN_DAYS` | `30` |
| `--retain-bytes` | `CROFT_RETAIN_BYTES` | `10737418240` (10 GiB) |

[Configuration](/reference/configuration/) explains each.

## top

```sh
croft top                 # refresh every 2 s
croft top --once          # print once and exit
croft top --interval 5
```

## stats

```sh
croft stats                          # the default dataset
croft stats research/data/nodes      # a directory of per-node directories
```

Reads plain files and zstd segments.

## profile

```sh
croft profile <container-id> --dataset research/data/nodes --from-ms <ms> --to-ms <ms>
```

The container id can be a prefix of at least 4 hex digits. Prints the nodes it ran on, the polls and processes seen, the share of polls holding a GPU, and the mean, p95 and max of CPU cores, memory, throttling and CPU pressure over the window.

## deploy

```sh
croft deploy docker [--image croft-collector] [--watch <cgroups>] [--dry-run]
croft deploy kubernetes [--image <image>] [--watch kubepods] [--dry-run]
```

`--dry-run` prints the `docker run` command or the manifest instead of applying it.

## collect

```sh
croft collect --namespace croft --out research/data/nodes
```

Copies each collector pod's closed segments and live file into `<out>/<node>/`. A live file smaller than the local copy, after a restart, is copied again from the start.

## scale

```sh
croft scale --namespace croft-exp --deployment webapp \
  --calls-per-request 2.06 --cpu-per-request-ms 50 --dry-run
```

Every poll it reads the collectors' `/metrics`, turns the syscall rate of the Deployment's pods into a request rate, and asks for `rate × CPU per request ÷ (CPU limit × target)` replicas, rounded up.

| Option | Default |
|---|---|
| `--syscall` | `sendto` |
| `--target` | `0.7`, as the HPA's |
| `--max-replicas` | `10` |
| `--confirm-polls` | `2` polls asking for more before scaling |
| `--interval` | `2` s |
| `--api` | `http://127.0.0.1:8001`, through `kubectl proxy` |
| `--dry-run` | log decisions without changing replicas |

`deploy/kubernetes/scaler.yaml` runs it next to a `kubectl proxy` sidecar, one per Deployment.
