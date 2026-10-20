---
title: Kubernetes
description: One collector per node as a DaemonSet, and pulling the datasets back.
sidebar:
  order: 2
---

Every node has its own kernel, so the collector runs as a DaemonSet: one instance per node, each writing its own dataset to the node's `/var/lib/croft`.

## Load the image

The manifest uses `croft-collector:dev`, which no registry holds. Build it and load it into each node's containerd. On a k3s cluster set up by [the lab playbook](/croft/guides/lab-cluster/):

```sh
docker build -f deploy/docker/Dockerfile -t croft-collector:dev .
cd labs/k3s-cluster
ansible-playbook load-image.yml -e image=croft-collector:dev -K
```

## Deploy

```sh
croft deploy kubernetes            # applies deploy/kubernetes/collector.yaml
croft deploy kubernetes --dry-run  # prints the manifest
kubectl -n croft get pods -o wide
```

or `kubectl apply -f deploy/kubernetes/collector.yaml`.

The DaemonSet runs privileged with `hostPID`, tolerates every taint and mounts tracefs, the cgroup tree and `/var/lib/croft`. Its environment:

| Variable | Value | Effect |
|---|---|---|
| `WATCH_CGROUPS` | `kubepods` | record every pod's processes, GPU or not |
| `CROFT_METRICS_ADDR` | `0.0.0.0:9464` | serve the latest poll per container at `/metrics`, for `croft scale` |
| `DATASET_PATH` | `/data/croft-dataset.jsonl` | the node's `/var/lib/croft` |

[Configuration](/croft/reference/configuration/) lists the others.

## Upgrade

Load the new image, then restart the pods so they pick it up:

```sh
kubectl -n croft rollout restart ds/croft-collector
```

## Pull the datasets

With the lab's Ansible inventory, `collect.yml` copies each agent's segments from the last 24 hours, and its live file, into `research/data/nodes/<node>/`:

```sh
cd labs/k3s-cluster
ansible-playbook collect.yml                      # last 24 hours
ansible-playbook collect.yml -e collect_hours=0   # every segment the agents keep
```

The server's own directory is linked, not copied. `croft stats research/data/nodes` summarizes what arrived.
