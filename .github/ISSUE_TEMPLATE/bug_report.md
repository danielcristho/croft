---
name: Bug Report
about: Report a bug in the collector, the croft CLI, the scaler or the experiment harness
title: "[BUG] "
labels: bug
assignees: ""
---

## Component 🧩

- [ ] Collector (eBPF program or userspace)
- [ ] `croft` CLI (`scale`, `profile`, `collect`, ...)
- [ ] Kubernetes manifests or Helm chart
- [ ] Experiment harness or load profile
- [ ] Other

## What Happened

Describe the bug and what you expected instead.

## Steps to Reproduce

1. Command or manifest you ran:

   ```sh

   ```

2. What you saw (logs with `RUST_LOG=debug` if possible):

   ```text

   ```

## Environment

- **croft version or commit**:
- **Kernel** (`uname -r`):
- **Kubernetes** (`kubectl version`), e.g. k3s v1.36.4:
- **Node and runtime** (CPU, GPU, containerd version):

## Checklist

- [ ] I have checked this is not already reported in another issue
- [ ] I have removed hosts, IP addresses and credentials from the logs
