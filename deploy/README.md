# Deploying the collector

The collector runs on each machine it observes, as root, on Linux only. Pick
one of the two paths below.

## Requirements

- Linux with eBPF and tracefs, which any recent distro kernel has. Check:
  `mount | grep tracefs` shows `/sys/kernel/tracing`.
- Root, or a privileged container.
- A GPU for real samples. NVIDIA (`/dev/nvidia*`) and DRM (`/dev/dri/*`) devices
  are both tracked; without either the collector runs but classifies nothing.

## Option A: Docker

No toolchain on the host. The image builds its own nightly, LLVM and
`bpf-linker`, so the first build takes a while.

```sh
cd croft
docker build -f deploy/docker/Dockerfile -t croft-collector .

sudo mkdir -p /var/lib/croft
docker run -d --name croft-collector --restart unless-stopped \
  --privileged --pid=host --cgroupns=host \
  -v /sys/kernel/tracing:/sys/kernel/tracing:ro \
  -v /var/lib/croft:/data \
  croft-collector

docker logs -f croft-collector
```

Each flag is needed: `--privileged` to load eBPF, `--pid=host` so backfill sees
host processes, `--cgroupns=host` so cgroup paths read as on the host, not
relative to the container, the tracefs mount so tracepoints can attach, and `/data` so the
dataset outlives the container.

The Dockerfile installs whichever nightly is current and pins `LLVM_VERSION=23`.
If a later nightly moves to another LLVM, the build fails at link time with
`Invalid record`; bump the argument to match `rustc +nightly --version --verbose`.

## Option B: native binary with systemd

### Toolchain, once per machine

```sh
rustup toolchain install nightly --component rust-src
rustc +nightly --version --verbose | grep LLVM   # note the major version, 23 here

wget https://apt.llvm.org/llvm.sh && chmod +x llvm.sh && sudo ./llvm.sh 23
LLVM_SYS_231_PREFIX=/usr/lib/llvm-23 PATH=/usr/lib/llvm-23/bin:$PATH \
  cargo install bpf-linker --locked
```

`just` is needed for the recipes: `cargo install just` or the distro package.

### Build and install

```sh
cd croft
just build-release
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo cp deploy/systemd/croft-collector.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now croft-collector

journalctl -u croft-collector -f
```

The dataset lands in `/var/lib/croft/croft-dataset.jsonl`.

### Upgrade

```sh
git pull
just build-release
sudo install -m 0755 collector/target/release/croft /usr/local/bin/croft
sudo systemctl restart croft-collector
```

## Trying it without installing

`just run` builds a debug binary and runs it under sudo in the foreground. The
dataset goes to `/var/lib/croft/croft-dataset.jsonl`, the same file the
deployments write, so `croft top` and `croft stats` read it without `--dataset`.

## Recording processes without a GPU

By default only processes holding a GPU device get dataset rows. `WATCH_CGROUPS`
adds every process in cgroups whose path contains one of its comma-separated
substrings, GPU or not:

```sh
WATCH_CGROUPS=kubepods just docker-up          # every Kubernetes pod, kind included
WATCH_CGROUPS=docker-<container-id> just docker-up
```

Those processes come from walking `/sys/fs/cgroup`, not from syscall activity, so
a CPU-bound loop that makes no syscalls is still recorded. Their rows carry
`"workload": "no-gpu"` and `null` GPU fields. Under systemd, set it with
`Environment=WATCH_CGROUPS=...` in the unit.

## Checking it works

Start the collector, then drive the GPU from another terminal, for example:

```sh
python3 -c "import torch, time; x = torch.zeros(1).cuda(); time.sleep(30)"
```

Within a poll interval (2s) the log shows:

```
22:31:07.412  INFO  reporter  workload=gpu-onset comm=<name> tgid=<pid> cpu=0.95 rss_mib=1412 ioctl=<n> syscalls=futex:282/s,...
```

and a line is appended to the dataset. The dataset gets a row for every process
holding a GPU device on every poll; the log shows `gpu-onset` rows at info and
`gpu-held` rows (no GPU ioctl in the last 5 s, often busy on mapped queues) at
debug. If `nvidia-smi` reports a driver/library
version mismatch, reboot first: CUDA does not open the device until the loaded
driver matches the library.

A `refused ... inserts` warning means a map filled and samples were lost; it is
logged each poll the count grows.

## Removing

```sh
# Docker
docker rm -f croft-collector

# systemd
sudo systemctl disable --now croft-collector
sudo rm /etc/systemd/system/croft-collector.service /usr/local/bin/croft
sudo systemctl daemon-reload
```

`/var/lib/croft` holds the dataset and is left in place.
