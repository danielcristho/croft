# croft experiments

Labelled workloads on the k3s lab cluster, recorded so croft's dataset can be
joined to them exactly. Two suites:

| Suite | Workloads | Recorded |
|---|---|---|
| `gpu` | `train-cnn`, `train-transformer`, `infer-batch-cnn`, `infer-generate-lm`, `serve-http`, `serve-llm-ollama`, `gpu-idle`, each a Kubernetes Job asking for one GPU | node, pod, container id, start and end from the container status |
| `web` | a CPU-bound HTTP service under Locust load, at fixed replicas or under an HPA | Locust history, Prometheus series (CPU, memory, network, replicas) for the same window |

croft's collector tags every row with the container id, so a run's rows are
found by that id rather than by time.

## Requirements

- The cluster from [`labs/k3s-cluster`](../../k3s-cluster): k3s, the NVIDIA
  device plugin, ingress-nginx on NodePort 30080, kube-prometheus-stack, and the
  collector DaemonSet with `WATCH_CGROUPS=kubepods`.
- `KUBECONFIG` pointing at the cluster, and `uv`.
- The webapp image on every node:
  ```sh
  docker build -t croft-webapp:v1 workloads/webapp
  (cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 --ask-become-pass)
  ```

## Usage

```sh
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml

uv run run.py gpu --dry-run                         # the plan: 7 workloads x 3 repetitions
uv run run.py gpu                                   # every GPU workload
uv run run.py gpu --only serve-llm-ollama --reps 1  # one workload
uv run run.py gpu --node gpu-node-1                # pin to one node

uv run run.py web --prefix r10 --replicas 10        # 10, 20, 40, 80 users at 10 fixed replicas
uv run run.py web --prefix hpa                      # the same levels under the HPA
uv run run.py web --prefix lead --steps 10,20,40,80 # one run climbing the steps, for lead time
```

Only one experiment runs at a time (`data/experiment.lock`): two workloads
sharing CPUs pollute each other's numbers.

After a session, pull croft's per-node datasets with
`ansible-playbook collect.yml` in `labs/k3s-cluster`.

## Output

```
data/
├── gpu/runs.jsonl            one line per run: label, params, node, container_id, started_ms, ended_ms
└── web/
    ├── runs.jsonl            one line per run: users or steps, replicas or hpa, window
    └── <run>/
        ├── locust_*.csv          stats, history, failures, exceptions
        └── prometheus.csv    cpu_cores, memory_bytes, network, replicas, ready and desired replicas
```

## Layout

| Path | What |
|---|---|
| `run.py` | command line for both suites |
| `experiment/` | `gpu` and `web` suites, `kube` (kubectl), `prometheus` (range queries), `records` (lock and run lines) |
| `workloads/gpu/` | the GPU workloads (PyTorch, ollama), mounted into Jobs through a ConfigMap |
| `workloads/webapp/` | the HTTP service, its image, manifest and HPA |
| `load/locustfile.py` | constant users, or steps from `LOAD_STEPS` |

## Notes

- Probe timeouts on the webapp are 5 s, not the 1 s default. Under CPU
  starvation a 1 s probe marks a slow pod dead, the kubelet restarts it, and the
  run records restarts instead of load.
- Locust runs with `--exit-code-on-error 0`: failed requests are data.
- The first run on a node pulls the PyTorch or ollama image; ollama models are
  cached per node in `/var/lib/croft/ollama`.

## Reference

The web suite's design (Locust load against a CPU-bound service, Prometheus
series exported for the measurement window, one run per user count) follows the
workload experiments in [rm77/cloud2023](https://github.com/rm77/cloud2023),
`kubernetes/research/experiments`. The code here is written for croft and does
not copy it.
