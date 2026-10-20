# croft experiments

This bundle runs **controlled workload experiments** on the k3s lab cluster and records them so croft's dataset can be joined to them exactly.

croft's collector tags every row with the container id, so a run's rows are found by that id rather than by time.

---

# 1. Purpose

The question behind the web experiments:

> Can an autoscaler use eBPF telemetry to detect a workload burst early enough to add replicas before the application degrades?

Each run changes the offered load and records, for the same window, what every party saw:

```text
offered load (open-loop RPS or Locust users)
       |
       v
web app pods ------------------------------+
       |                                   |
       +--> kernel: syscalls, PSI,         +--> client side: latency, failures
       |    throttling (croft, every 2 s)       (workload.csv, locust_*.csv)
       |
       +--> CPU utilization (HPA, every 15 s) --> replicas (timeline.json)
       |
       +--> Prometheus series (prometheus.csv)
```

The analysis then asks which signal crossed its threshold first, how long the HPA took, and when latency broke its objective (p95 above 500 ms).

---

# 2. Experiment suites

| Suite | Workloads | Recorded |
|---|---|---|
| `gpu` | `train-cnn`, `train-transformer`, `infer-batch-cnn`, `infer-generate-lm`, `serve-http`, `serve-llm-ollama`, `gpu-idle`, each a Kubernetes Job asking for one GPU | node, pod, container id, start and end from the container status |
| `web` | a CPU-bound HTTP service under Locust load or open-loop RPS, at fixed replicas or under an HPA | client-side latency, Prometheus series (CPU, memory, network, replicas), HPA and pod timeline for the same window |

The web suite has three kinds of load:

| Load | Command | Typical research purpose |
|---|---|---|
| Locust users, fixed levels | `--users 10 20 40 80` | capacity per replica count |
| Locust users, steps or ramp | `--steps 2,4,8,12 [--shape ramp]` | lead time of a slow rise |
| Open-loop RPS profile | `--profile constant\|ramp\|burst\|periodic\|variable` | transient response to a burst |

Use open-loop load for bursts. A Locust user waits for each answer before sending the next request, so once the app saturates, more users mean longer queues, not more requests, and the burst hides behind the app's own slowness.

---

# 3. Requirements

- The cluster from [`labs/k3s-cluster`](../../k3s-cluster): k3s, the NVIDIA device plugin, ingress-nginx on NodePort 30080, kube-prometheus-stack, and the collector DaemonSet with `WATCH_CGROUPS=kubepods`.
- `KUBECONFIG` pointing at the cluster, and `uv`.
- The webapp image on every node (see step 2 below).

---

# 4. Directory structure

```text
labs/research/experiment/
├── README.md
├── run.py                  command line for both suites
├── experiment/             gpu and web suites, kube (kubectl), prometheus (range queries),
│                           records (lock and run lines), profiles and rps (open-loop load)
├── workloads/
│   ├── gpu/                the GPU workloads (PyTorch, ollama), mounted into Jobs through a ConfigMap
│   ├── webapp/             the HTTP service, its image, manifest and HPA
│   └── linkerd/            PodMonitor for the Linkerd proxies
├── load/locustfile.py      constant users, or steps from LOAD_STEPS
└── data/                   results, one directory per run
```

---

# 5. Running a burst experiment

## Step 1: point at the cluster

```sh
cd labs/research/experiment
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml
kubectl get nodes
```

Every node should be `Ready`.

## Step 2: load the webapp image on every node

```sh
docker build -t croft-webapp:v1 workloads/webapp
(cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 --ask-become-pass)
```

The harness checks the image on every node before a run. After loading, the node status can take a few minutes to list it.

## Step 3: run the burst

```sh
for i in 1 2 3; do
  uv run run.py web --prefix burst-$i --profile burst --scale 0.5 || break
done
```

Each run:

```text
deploy webapp at 1 replica, apply the HPA
       |
       v
warm-up, 30 s at 5 RPS
       |
       v
burst profile, 600 s (at --scale 0.5):
  5 RPS for 180 s  ->  75 RPS for 120 s  ->  5 RPS again
       |
       v
export Prometheus series, HPA events and pod timeline
       |
       v
cool-down, 15 s
```

`--scale 0.5` keeps the burst within ten pods: at 45 to 65 ms of CPU per request, 75 RPS needs about 6 to 7 pods at the HPA's 70% target.

## Step 4: pull croft's datasets

```sh
(cd ../../k3s-cluster && ansible-playbook collect.yml)
```

Each node's dataset lands in `research/data/nodes/<node>.jsonl`: the last 24 hours of each agent, and a link to the server's file. `-e collect_hours=48` widens the window; `-e collect_hours=0` copies whole files, which older bursts need.

## Step 5: analyse

Open `research/burst_signals.ipynb` (and `research/lead_time.ipynb` for the step and ramp runs).

---

# 6. Other runs

```sh
uv run run.py gpu --dry-run                         # the plan: 7 workloads x 3 repetitions
uv run run.py gpu                                   # every GPU workload
uv run run.py gpu --only serve-llm-ollama --reps 1  # one workload
uv run run.py gpu --node gpu-node-1                 # pin to one node

uv run run.py web --prefix r10 --replicas 10        # 10, 20, 40, 80 users at 10 fixed replicas
uv run run.py web --prefix hpa                      # the same levels under the HPA
uv run run.py web --prefix lead --steps 2,4,8,12    # one run climbing the steps, for lead time
uv run run.py web --prefix lead --steps 2,4,8,12 --shape ramp  # the same counts, one user at a time
uv run run.py web --prefix cd --profile ramp --scale 0.3       # an open-loop ramp at 30% of its rates
uv run run.py web --prefix duo --profile burst --scale 0.5 --scaler croft  # croft scale next to the HPA
uv run run.py calibrate                             # CPU per request on each node, into data/node-speed.json
```

Only one experiment runs at a time (`data/experiment.lock`): two workloads sharing CPUs pollute each other's numbers.

---

# 7. Output

```text
data/
├── gpu/runs.jsonl            one line per run: label, params, node, container_id, started_ms, ended_ms
└── web/
    ├── runs.jsonl            one line per run: users, steps or profile, replicas or hpa, window, containers
    └── <run>/
        ├── locust_*.csv      Locust runs: stats, history, failures, exceptions
        ├── workload.csv      open-loop runs: target and achieved RPS, latency per second
        ├── prometheus.csv    cpu_cores, cpu_throttled_s, memory_bytes, network, restarts, replicas
        ├── scaler.log        --scaler croft: the scaler's decisions, with timestamps
        └── timeline.json     HPA rescales, replica set changes from any scaler, pod created and Ready times
```

---

# 8. Notes

- Probe timeouts on the webapp are 5 s, not the 1 s default. Under CPU starvation a 1 s probe marks a slow pod dead, the kubelet restarts it, and the run records restarts instead of load.
- The webapp requests what it is limited to, one core, so the HPA's 70% target is 0.7 core per pod. With a 100m request ten pods hold only 0.7 core and the HPA reaches its maximum on the first step.
- One Locust user costs about 0.5 core (45 ms of CPU per request). Steps 2, 4, 8, 12 ask the HPA for about 2, 3, 6 and 9 pods; beyond 16 users ten pods saturate.
- A step jumps the load at once, so every signal reacts within a poll and croft can lead the SLO by a poll at most; the ramp spreads the same rise over each step and shows lead time.
- Locust runs with `--exit-code-on-error 0`: failed requests are data.
- `--profile` follows cloud2023's `cloud-dynamics-experiments`: open-loop requests per second (constant, ramp, burst, periodic, variable) with its scenario parameters, a 30 s warm-up at 5 RPS and a 15 s cool-down, and a `workload.csv` with its columns. The webapp costs 40 to 65 ms of CPU per request against cloud2023's lighter `cpu-bound` app, so its rates may need `--scale` to stay within ten pods.
- `apps.json` lists the applications under test: namespace, Deployment, container, manifest folder (`k8s.yaml` and `hpa.yaml`), host, request and health paths, and the calibration `croft scale` needs (syscall, calls and CPU milliseconds per request). `--app` picks one; a new app is a new entry, not new code.
- `--scaler croft` deploys `deploy/kubernetes/scaler.yaml` for the app before the warm-up, saves its log and removes it after the run. It needs the collector DaemonSet with `CROFT_METRICS_ADDR` set and an image that has `croft scale`.
- The first run on a node pulls the PyTorch or ollama image; ollama models are cached per node in `/var/lib/croft/ollama`.

---

# 9. Reference

The web suite's design (load against a CPU-bound service, Prometheus series exported for the measurement window, one run per load level) follows the workload experiments in [rm77/cloud2023](https://github.com/rm77/cloud2023), `kubernetes/research/experiments`; the open-loop profiles follow `kubernetes/research/cloud-dynamics-experiments` in the same repository. The code here is written for croft and does not copy it.
