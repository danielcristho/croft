---
title: Kubernetes
description: One collector per node as a DaemonSet, and pulling the datasets back.
sidebar:
  order: 2
---

Every node has its own kernel, so the collector runs as a DaemonSet: one instance per node, each writing its own dataset to the node's `/var/lib/croft`.

## Install with Helm

CI publishes the image to `ghcr.io/danielcristho/croft` on every push to `main` (tag `main`) and on version tags. The chart installs the collector on every node:

```sh
helm install croft deploy/helm/croft -n croft --create-namespace
kubectl -n croft get pods -o wide
```

`scaler.targets` is empty by default, so the chart starts no `croft scale`. To let croft scale a Deployment, add one entry per Deployment in your own values file:

```yaml
# my-values.yaml
scaler:
  targets:
    - name: my-api            # names the croft scale Deployment
      namespace: shop         # where the Deployment runs
      deployment: api         # the Deployment to scale
      syscall: write          # the syscall counted per request
      callsPerRequest: 8.0    # calls of that syscall per request, on average
      cpuPerRequestMs: 2.5    # CPU one request costs, on average
      dryRun: true            # log decisions only; remove to scale
```

```sh
helm upgrade --install croft deploy/helm/croft -n croft -f my-values.yaml
kubectl -n croft logs deploy/croft-scaler-my-api -c scaler -f
```

Each entry starts its own `croft scale` Deployment. Remove the Deployment's HPA before turning `dryRun` off, or the two will fight over the replica count. An entry can also set `target` (default 0.7), `maxReplicas` (10), `confirmPolls` (2), `interval` (2 s) and `scaleDownWindow` (300 s). `collector.podMonitor.enabled: true` adds a PodMonitor for the Prometheus Operator; `values.yaml` lists the other chart-wide options.

`ci/targets-values.yaml` in the chart is the input CI renders it with, not a list of supported workloads.

### What the chart creates

The collector DaemonSet keeps the label `app: croft-collector`. `croft scale` finds the collectors through that label, so renaming it breaks scaling:

```yaml
# templates/collector.yaml
metadata:
  name: croft-collector
  labels:
    app: croft-collector
```

```text
--collectors-selector    app=croft-collector    croft scale's default
```

Each target runs `croft scale` beside a `kubectl proxy` sidecar. `croft scale` reads pods and patches the Deployment's scale through the proxy on `127.0.0.1:8001`, so it speaks plain HTTP and holds no Kubernetes credentials itself:

```yaml
# templates/scaler.yaml, per target
containers:
  - name: scaler
  - name: proxy
    image: registry.k8s.io/kubectl:v1.36.4
    args: ["proxy", "--port=8001", "--address=127.0.0.1"]
```

The proxy acts with the release's `<release>-scaler` ServiceAccount, bound to a ClusterRole that allows only:

| Resource | Verbs |
|---|---|
| `pods` | `get`, `list` |
| `deployments` | `get` |
| `deployments/scale` | `get`, `patch` |

### Measure the three numbers

`syscall`, `callsPerRequest` and `cpuPerRequestMs` differ per application. They are averages, so they are rarely whole numbers: a request may write 8 times or 9, and logs or health checks add calls that no request made.

Measure them on one replica with no autoscaler. Send constant load at a known rate `R` for 3 minutes, at three rates such as 20, 50 and 100 RPS, and read the collector's signals for the pod's container from Prometheus:

```promql
avg_over_time(croft_container_syscall_rate{container_id="<id>"}[2m]) / R
avg_over_time(croft_container_cpu_cores{container_id="<id>"}[2m]) * 1000 / R
```

The first gives calls per request for every syscall, the second the CPU milliseconds per request. The container id is the part after `containerd://` in `kubectl get pod <pod> -o jsonpath='{.status.containerStatuses[0].containerID}'`.

Choose the syscall whose calls per request change least across the three rates, and take its mean as `callsPerRequest`. Take the CPU per request at the highest rate as `cpuPerRequestMs`, since a fixed idle cost inflates it at low rates.

`croft scale` turns them into replicas. Hotel Reservation's frontend uses `write`, 11.4 calls and 1.6 ms per request; with 2,280 `write` calls a second, a 200m limit and a 70% target:

```text
requests/s = 2280 / 11.4                  = 200
replicas   = ceil(200 x 1.6 ms / (200m x 0.7))
           = ceil(0.32 / 0.14)            = 3
```

:::caution
The numbers hold while the mix of requests stays the same. If clients start calling a path with more or fewer syscalls per request, croft misjudges the request rate. Calibrate again when the traffic mix changes.
:::

## Load the image

The manifests in `deploy/kubernetes/` use `croft-collector:dev`, which no registry holds. Build it and load it into each node's containerd. On a k3s cluster set up by [the lab playbook](/guides/lab-cluster/):

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

[Configuration](/reference/configuration/) lists the others.

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
