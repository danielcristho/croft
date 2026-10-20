---
name: croft
description: Working knowledge for croft, the eBPF collector and proactive autoscaling project, and its k3s lab experiments. Use it for any croft task, whether building or changing the collector or CLI, running burst, ramp or step experiments with the harness, collecting node datasets, analysing signals in the notebooks, deploying test apps on the lab cluster, writing commits, or explaining results. Use it even when the request only mentions HPA, burst detection, eBPF syscalls, lead time, Online Boutique or cloud2023 in a croft context.
---

# croft

croft reads what processes do in the kernel through eBPF (syscall rates, CPU,
pressure, throttling, GPU use) and aims to add replicas before a workload burst
degrades the application. It is a Cloud Computing course final project:
*Proactive Kubernetes Autoscaling Using eBPF Telemetry for Early Workload Burst
Detection in Web Services*.

Built: the eBPF collector, the dataset, the `croft` CLI, the experiment harness,
the analysis notebooks. Not built: the burst detector and the scaler.

## Read next

| File | Read it when |
|---|---|
| [code-style.md](code-style.md) | writing or reviewing any code: Clean Code, SOLID, DRY, comments |
| [commits.md](commits.md) | proposing a commit message |
| [cluster.md](cluster.md) | touching the lab cluster, node images or node access |
| [experiments.md](experiments.md) | running experiments or collecting datasets |
| [signals.md](signals.md) | reading the dataset, choosing signals, running notebooks, citing results |

## Toolchain

```sh
just build    # userspace and eBPF program
just test     # cargo test
just lint     # fmt --check and clippy
just fmt      # apply formatting
```

The eBPF crate needs nightly Rust and `bpf-linker` against a matching LLVM;
`deploy/docker/Dockerfile` shows the setup. Userspace-only work does not.
Enable the hooks once per clone: `git config core.hooksPath .githooks`.

```sh
croft run       # the collector; needs root
croft top       # processes of the newest poll
croft stats     # dataset summary
croft profile   # resource profile of a container over a window, as JSON
croft deploy    # start the collector on Docker or Kubernetes
croft collect   # pull each node's segments and live file into <out>/<node>/, from the DaemonSet
```

The default dataset is `/var/lib/croft/croft-dataset.jsonl`; older rows sit in zstd segments `croft-dataset-<UTC start>.jsonl.zst` beside it.

## Applications under test

| App | Role |
|---|---|
| `webapp` (`labs/research/experiment/workloads/webapp`) | controlled: fixed CPU per request at `/cpu` |
| Online Boutique v0.10.7 (`labs/research/apps/online-boutique`) | realistic: 11 microservices calling each other |
| cloud2023 `cpu-bound` / `stateless-api` | the course framework's own apps |

LLM inference autoscaling is croft's next research, not this project.

## Working with the user

- Reply in Indonesian, plain and short: the idea first, little jargon. Code,
  comments and commit messages stay in English.
- Ask before committing; never push unless asked. The user often commits
  themselves.
- Never delete or reword comments the user wrote.
- Ask before deleting files, cluster resources or images the user made.
- Real hosts, addresses, usernames and passwords stay out of committed files and
  command lines.
- Do not send the user's email address to external services.
- Report results as measured. A failed run, a masked exit code or a threshold
  chosen after seeing the data is said plainly.
