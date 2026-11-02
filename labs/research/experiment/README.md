# croft experiments

This harness runs controlled workload experiments on the k3s lab cluster and records each one so croft's dataset can be joined to it exactly. croft tags every row with the container id, so a run's rows are found by that id rather than by time.

The web experiments ask one question: can an autoscaler use eBPF telemetry to see a burst early enough to add replicas before the application degrades? Each run changes the offered load and records, for the same window, what the client, the kernel (through croft), the HPA and Prometheus saw. The analysis then asks which signal crossed its threshold first, how long the autoscaler took, and when p95 latency broke its 500 ms objective.

The step-by-step procedure for the web applications is in [RUNNING.md](RUNNING.md).

**Table of contents**

- [Suites](#suites)
- [Prerequisites](#prerequisites)
- [Commands](#commands)
- [Applications](#applications)
- [Output](#output)
- [Design notes](#design-notes)
- [Reference](#reference)

## Suites

| Suite | What runs | What is recorded |
|---|---|---|
| `gpu` | `train-cnn`, `train-transformer`, `infer-batch-cnn`, `infer-generate-lm`, `serve-http`, `serve-llm-ollama` and `gpu-idle`, each a Kubernetes Job asking for one GPU | node, pod, container id, start and end from the container status |
| `web` | an application of `apps.json` under Locust users or open-loop RPS, at fixed replicas or under an autoscaler | client latency, Prometheus series, HPA and pod events for the same window |
| `calibrate` | the webapp pinned to each node in turn | CPU per request per node, into `data/node-speed.json` |
| `compare` | nothing; it reads finished burst runs | one scored row per run, mean ± sd per arm |

The web suite takes three kinds of load:

- **Locust users at fixed levels** (`--users 10 20 40 80`): capacity per replica count.
- **Locust users in steps or a ramp** (`--steps 2,4,8,12 [--shape ramp]`): lead time of a slow rise.
- **An open-loop RPS profile** (`--profile constant|ramp|burst|periodic|variable`): the response to a burst.

Use open-loop load for bursts. A Locust user waits for each answer before sending the next request, so once the application saturates, more users mean longer queues rather than more requests, and the burst hides behind the application's own slowness.

## Prerequisites

- The cluster from [`labs/k3s-cluster`](../../k3s-cluster): k3s, the NVIDIA device plugin, ingress-nginx on NodePort 30080, kube-prometheus-stack, and the croft collector DaemonSet with `WATCH_CGROUPS=kubepods`.
- `KUBECONFIG` pointing at the cluster, and [`uv`](https://docs.astral.sh/uv/).
- Each application's images on every node. See [workloads/README.md](workloads/README.md).

```bash
cd labs/research/experiment
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml
```

## Commands

```bash
# GPU suite
uv run run.py gpu --dry-run                          # the plan: 7 workloads x 3 repetitions
uv run run.py gpu                                    # every GPU workload
uv run run.py gpu --only serve-llm-ollama --reps 1   # one workload
uv run run.py gpu --node gpu-node-1                  # pin to one node

# Web suite, Locust
uv run run.py web --prefix r10 --replicas 10         # 10, 20, 40, 80 users at 10 fixed replicas
uv run run.py web --prefix hpa                       # the same levels under the HPA
uv run run.py web --prefix lead --steps 2,4,8,12     # one run climbing the steps
uv run run.py web --prefix lead --steps 2,4,8,12 --shape ramp   # the same counts, one user at a time

# Web suite, open loop
uv run run.py web --prefix cd --profile ramp --scale 0.3                    # a ramp at 30% of its rates
uv run run.py web --prefix duo --profile burst --scale 0.5 --scaler croft   # croft scale next to the HPA
uv run run.py web --app hotel-reservation --prefix hr --profile burst --scale 2 --scaler keda   # KEDA instead of the HPA

# Calibration and scoring
uv run run.py calibrate                              # CPU per request on each node
uv run run.py compare --prefix hpa- duo-             # burst runs scored per arm
```

Only one experiment runs at a time (`data/experiment.lock`): two workloads sharing CPUs pollute each other's numbers.

## Applications

`apps.json` lists the applications under test. A new application is a new entry, not new code; `--app` picks one.

| Field | Meaning |
|---|---|
| `namespace`, `deployment`, `container` | where the application runs; the harness selects its pods with `app=<deployment>` |
| `manifests` | a folder with `k8s.yaml` for the application and `hpa.yaml` for its baseline autoscaler |
| `host`, `path`, `health` | the ingress host, the request path the load hits, and the readiness check |
| `scaler` | the calibration croft scale and KEDA need: `syscall`, `calls_per_request`, `cpu_per_request_ms` |
| `scale` | the `--profile` scale a burst runs at by default |

Three applications are defined; [workloads/README.md](workloads/README.md) covers deploying them:

- **`webapp`**: a CPU-bound Python service, 40 to 65 ms of CPU per request, one core per pod.
- **`hotel-reservation`**: [DeathStarBench](https://github.com/delimitrou/DeathStarBench) hotelReservation. Every request is a hotel search that crosses five Go services. The frontend costs 1.6 ms of CPU per request and is limited to 200m.
- **`otel-demo`**: the [OpenTelemetry Demo](https://github.com/open-telemetry/opentelemetry-demo) 3.1.0. Every request asks for recommendations, which crosses Node.js, Python, Go and C++ services. Not calibrated yet.

The workload folders:

| Folder | Content |
|---|---|
| `workloads/gpu/` | the GPU workloads (PyTorch, ollama), mounted into Jobs through a ConfigMap |
| `workloads/webapp/` | the service, its image, manifest and HPA |
| `workloads/hotel-reservation/` | 19 Deployments, their Services, the ingress and the frontend's HPA |
| `workloads/otel-demo/` | OpenTelemetry Demo 3.1.0: 15 Deployments, a collector that drops every signal, the frontend's HPA |
| `workloads/ingress-nginx/` | the PodMonitor for the ingress request counts, KEDA's signal |
| `workloads/linkerd/` | the PodMonitor for the Linkerd proxies |

## Output

Everything lands under `data/`:

| Path | Content |
|---|---|
| `gpu/runs.jsonl` | one line per GPU run: label, parameters, node, container id, start and end |
| `web/runs.jsonl` | one line per web run: app, load, autoscaler, window, containers |
| `web/compare.csv` | `run.py compare`: one scored row per burst run |
| `web/<run>/workload.csv` | open-loop runs: target and achieved RPS, latency, per second |
| `web/<run>/locust_*.csv` | Locust runs: stats, history, failures, exceptions |
| `web/<run>/prometheus.csv` | `cpu_cores`, `cpu_throttled_fraction`, `memory_bytes`, network, `restarts`, replicas |
| `web/<run>/timeline.json` | HPA rescales, replica set changes from any scaler, pod created and Ready times |
| `web/<run>/scaler.log` | `--scaler croft`: croft's estimate every poll and its decisions |

croft's own datasets are pulled separately with `ansible-playbook collect.yml` from `labs/k3s-cluster` into `research/data/nodes/<node>/`, and analysed in `research/burst_signals.ipynb` and `research/lead_time.ipynb`.

## Design notes

- **Probe timeouts are 5 s, not the 1 s default.** Under CPU starvation a 1 s probe marks a slow pod dead, the kubelet restarts it, and the run records restarts instead of load.
- **Requests equal limits.** The webapp asks for the one core it is limited to, so the HPA's 70% target means 0.7 core per pod; with a 100m request, ten pods would hold 0.7 core between them. The hotel frontend's 200m follows the same rule at its own cost per request.
- **One Locust user costs about 0.5 core** on the webapp. Steps 2, 4, 8 and 12 ask the HPA for about 2, 3, 6 and 9 pods; beyond 16 users ten pods saturate.
- **A step jumps the load at once**, so every signal reacts within a poll and croft can lead the objective by one poll at most. A ramp spreads the same rise and shows lead time.
- **Failed requests are data.** Locust runs with `--exit-code-on-error 0`.
- **Throttling is a share of CFS periods.** kube-prometheus-stack drops `container_cpu_cfs_throttled_seconds_total`, so `cpu_throttled_fraction` divides throttled periods by all periods.
- **The open-loop profiles follow cloud2023.** `--profile` uses the rates of `cloud-dynamics-experiments`, a 30 s warm-up at 5 RPS and a 15 s cool-down, and writes `workload.csv` with the same columns. Both applications need `--scale` to keep a burst within ten pods.
- **`--scaler croft`** deploys `deploy/kubernetes/scaler.yaml` for the application before the warm-up, saves its log and removes it after the run. It needs the collector with `CROFT_METRICS_ADDR` set.
- **`--scaler keda`** applies a ScaledObject instead of `hpa.yaml`, with the same capacity per pod as croft and the HPA. Every deploy removes the previous run's ScaledObject.
- **GPU images are pulled on first use.** ollama models are cached per node in `/var/lib/croft/ollama`.

## Reference

The web suite's design (load against a CPU-bound service, Prometheus series exported for the measurement window, one run per load level) follows the workload experiments in [rm77/cloud2023](https://github.com/rm77/cloud2023), `kubernetes/research/experiments`, and its open-loop profiles follow `kubernetes/research/cloud-dynamics-experiments` in the same repository. The code here is written for croft and does not copy it.
