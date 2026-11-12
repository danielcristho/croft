# croft Collector — Deployment

This folder provides the ways to run the **croft collector** on a machine it observes.

The collector runs on each machine, as root, on Linux only. It loads eBPF programs, tracks every process holding a GPU device, and writes one dataset row per process per poll.

Three paths are available:

| Path | Toolchain on the host | Typical use |
|---|---|---|
| Docker | none; the image builds its own | a quick install, or a host without Rust |
| Native binary with systemd | Rust nightly, LLVM, `bpf-linker` | a long-running collector |
| Release binary | none; download it, as in the [README](../README.md#the-cli) | the CLI on your machine, or the systemd unit without building |

On Kubernetes, the collector runs as a DaemonSet, installed with the Helm chart in `deploy/helm/croft` or from `deploy/kubernetes/collector.yaml`.

## Directory structure

```text
deploy/
├── README.md
├── docker/
│   └── Dockerfile
├── systemd/
│   └── croft-collector.service
├── helm/
│   └── croft/              the collector, and croft scale for the targets in values.yaml
└── kubernetes/
    ├── collector.yaml      the collector as a DaemonSet
    └── scaler.yaml         croft scale, for the experiments
```

---

# 1. Prerequisites

The host must provide:

```text
Linux with eBPF and tracefs
root, or a privileged container
```

Any recent distribution kernel has eBPF and tracefs.

Check tracefs:

```sh
mount | grep tracefs
```

The output must show `/sys/kernel/tracing`.

For real samples the host needs a GPU. Both device kinds are tracked:

```text
/dev/nvidia*    NVIDIA
/dev/dri/*      DRM
```

Without either, the collector runs but classifies nothing.

---

# 2. Run with Docker

No toolchain is needed on the host. The image builds its own nightly, LLVM and `bpf-linker`, so the first build takes a while.

Build the image:

```sh
cd croft
docker build -f deploy/docker/Dockerfile -t croft-collector .
```

Create the data directory:

```sh
sudo mkdir -p /var/lib/croft
```

Start the collector:

```sh
docker run -d --name croft-collector --restart unless-stopped \
  --privileged --pid=host --cgroupns=host \
  -v /sys/kernel/tracing:/sys/kernel/tracing:ro \
  -v /var/lib/croft:/data \
  croft-collector
```

Follow its log:

```sh
docker logs -f croft-collector
```

Each flag is needed:

| Flag | Why |
|---|---|
| `--privileged` | to load eBPF |
| `--pid=host` | so backfill sees host processes |
| `--cgroupns=host` | so cgroup paths read as on the host, not relative to the container |
| `-v /sys/kernel/tracing` | so tracepoints can attach |
| `-v /var/lib/croft:/data` | so the dataset outlives the container |

The Dockerfile installs whichever nightly is current and pins `LLVM_VERSION=23`.

If a later nightly moves to another LLVM, the build fails at link time with:

```text
Invalid record
```

Bump the argument to match:

```sh
rustc +nightly --version --verbose | grep LLVM
```

---

# 3. Install the toolchain for a native build

Do this once per machine.

Install the nightly toolchain:

```sh
rustup toolchain install nightly --component rust-src
```

Note its LLVM major version, 23 here:

```sh
rustc +nightly --version --verbose | grep LLVM
```

Install that LLVM and `bpf-linker`:

```sh
wget https://apt.llvm.org/llvm.sh && chmod +x llvm.sh && sudo ./llvm.sh 23
LLVM_SYS_231_PREFIX=/usr/lib/llvm-23 PATH=/usr/lib/llvm-23/bin:$PATH \
  cargo install bpf-linker --locked
```

Install `just`:

```sh
cargo install just
```

or use the distribution package.

---

# 4. Build and install with systemd

Build the release binary:

```sh
cd croft
just build-release
```

Install it and its unit:

```sh
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo cp deploy/systemd/croft-collector.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now croft-collector
```

Follow its log:

```sh
journalctl -u croft-collector -f
```

The dataset lands in:

```text
/var/lib/croft/croft-dataset.jsonl
```

---

# 5. Try it without installing

Build a debug binary and run it under sudo, in the foreground:

```sh
just run
```

The dataset goes to the same file the deployments write:

```text
/var/lib/croft/croft-dataset.jsonl
```

so `croft top` and `croft stats` read it without `--dataset`.

---

# 6. Verify the collector

Start the collector, then drive the GPU from another terminal:

```sh
python3 -c "import torch, time; x = torch.zeros(1).cuda(); time.sleep(30)"
```

Within a poll interval (2 s), a successful log resembles:

```text
22:31:07.412  INFO  reporter  workload=gpu-onset comm=<name> tgid=<pid> cpu=0.95 rss_mib=1412 ioctl=<n> syscalls=futex:282/s,...
```

and a line is appended to the dataset.

The dataset gets a row for every process holding a GPU device on every poll.

The log shows:

```text
gpu-onset rows    at info
gpu-held rows     at debug; no GPU ioctl in the last 5 s, often busy on mapped queues
```

If `nvidia-smi` reports a driver/library version mismatch, reboot first. CUDA does not open the device until the loaded driver matches the library.

A warning like:

```text
refused ... inserts
```

means a map filled and samples were lost. It is logged each poll the count grows.

---

# 7. Record processes without a GPU

By default only processes holding a GPU device get dataset rows.

`WATCH_CGROUPS` adds every process in cgroups whose path contains one of its comma-separated substrings, GPU or not:

```sh
WATCH_CGROUPS=kubepods just docker-up                   # every Kubernetes pod, kind included
WATCH_CGROUPS=docker-<container-id> just docker-up
```

Those processes come from walking `/sys/fs/cgroup`, not from syscall activity, so a CPU-bound loop that makes no syscalls is still recorded.

Their rows carry:

```text
"workload": "no-gpu"
null GPU fields
```

Under systemd, set it in the unit:

```ini
Environment=WATCH_CGROUPS=kubepods
```

---

# 8. Rotation and retention

Every hour, or at 1 GiB, the collector:

```text
renames the live file to croft-dataset-<UTC start>.jsonl
compresses it with zstd on a separate thread (about 13x smaller)
deletes segments older than 30 days
deletes the oldest segments while all of them exceed 10 GiB
```

Four variables change the limits. 0 turns one off.

```text
CROFT_ROTATE_SECS
CROFT_ROTATE_BYTES
CROFT_RETAIN_DAYS
CROFT_RETAIN_BYTES
```

A live file left by an older collector becomes a segment on the first poll.

`croft stats` and `croft profile` read the segments directly. Elsewhere, `zstd -dc` or Python's `zstandard` decompresses them.

---

# 9. Upgrade

```sh
git pull
just build-release
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo systemctl restart croft-collector
```

---

# 10. Remove

Docker:

```sh
docker rm -f croft-collector
```

systemd:

```sh
sudo systemctl disable --now croft-collector
sudo rm /etc/systemd/system/croft-collector.service /usr/local/bin/croft
sudo systemctl daemon-reload
```

`/var/lib/croft` holds the dataset and is left in place.

---

# 11. Kubernetes with Helm

The chart in `deploy/helm/croft` installs the collector on every node. The image is `ghcr.io/danielcristho/croft`, which CI publishes on every push to `main`.

```sh
helm install croft oci://ghcr.io/danielcristho/charts/croft -n croft --create-namespace
kubectl -n croft get pods -o wide
```

The chart from GHCR pulls the image of the same release. From a clone, `deploy/helm/croft` installs the chart of `main`, with the `main` image.

## 11.1 Scale your own Deployment

`scaler.targets` is empty by default, so the chart starts no `croft scale`. Add one entry per Deployment in your own values file:

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
helm upgrade --install croft oci://ghcr.io/danielcristho/charts/croft -n croft -f my-values.yaml
kubectl -n croft logs deploy/croft-scaler-my-api -c scaler -f
```

Each entry starts its own `croft scale` Deployment. Remove the Deployment's HPA before turning `dryRun` off, or the two will fight over the replica count. An entry can also set `target` (default 0.7), `maxReplicas` (10), `confirmPolls` (2), `interval` (2 s) and `scaleDownWindow` (300 s). `values.yaml` lists the chart-wide options, such as `collector.podMonitor.enabled`.

`ci/targets-values.yaml` is the input CI renders the chart with. It is not a list of supported workloads.

## 11.2 Measure the three numbers

`syscall`, `callsPerRequest` and `cpuPerRequestMs` differ per application and are averages, so they are rarely whole numbers. Measure them on one replica with no autoscaler:

```text
1. scale the Deployment to 1 replica and remove its HPA
2. send constant load at a known rate R, for example 20 RPS, for 3 minutes
3. repeat at two more rates, for example 50 and 100 RPS
```

At each rate, read the collector's signals for the pod's container, from Prometheus with `collector.podMonitor.enabled: true`:

```promql
avg_over_time(croft_container_syscall_rate{container_id="<id>"}[2m]) / R
avg_over_time(croft_container_cpu_cores{container_id="<id>"}[2m]) * 1000 / R
```

The first gives calls per request for every syscall, the second the CPU milliseconds per request. `kubectl get pod <pod> -o jsonpath='{.status.containerStatuses[0].containerID}'` gives the container id after `containerd://`.

Choose the syscall whose calls per request change least across the three rates, and take its mean as `callsPerRequest`. Take the CPU per request at the highest rate as `cpuPerRequestMs`, since a fixed idle cost inflates it at low rates.

The experiment harness does the same for its three applications with `just samples calibrate <app>`; see [RUNNING.md section 8](../labs/research/experiment/RUNNING.md#8-calibrate-an-application).

> **Note**
> The numbers hold while the mix of requests stays the same. If clients start calling a path with more or fewer syscalls per request, croft misjudges the request rate. Calibrate again when the traffic mix changes.
