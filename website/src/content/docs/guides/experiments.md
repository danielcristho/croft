---
title: Experiments
description: Run a burst against a web service, with the HPA as the baseline.
sidebar:
  order: 5
---

The harness in `labs/research/experiment` runs controlled workloads on the [lab cluster](/croft/guides/lab-cluster/) and records them so croft's dataset joins them exactly: every row carries the container id, so a run's rows are found by id, not by time.

The question behind the web experiments:

> Can an autoscaler use eBPF telemetry to detect a workload burst early enough to add replicas before the application degrades?

```text
offered load (open-loop RPS or Locust users)
       |
       v
web app pods ------------------------------+
       |                                   |
       +--> kernel: syscalls, PSI,         +--> client side: latency, failures
       |    throttling (croft, every 2 s)
       +--> CPU utilization (HPA, every 15 s) --> replicas
       +--> Prometheus series
```

## Suites

| Suite | Workloads |
|---|---|
| `gpu` | training, batch inference, generation, HTTP and ollama serving, an idle GPU; each a Job asking for one GPU |
| `web` | a CPU-bound HTTP service under Locust users or open-loop RPS, at fixed replicas or under an HPA |

Use open-loop load for bursts. A Locust user waits for each answer before sending the next request, so once the app saturates, more users mean longer queues, not more requests.

## Run a burst

```sh
cd labs/research/experiment
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml

docker build -t croft-webapp:v1 workloads/webapp
(cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 -K)

for i in 1 2 3; do
  uv run run.py web --prefix burst-$i --profile burst --scale 0.5 || break
done

(cd ../../k3s-cluster && ansible-playbook collect.yml)
```

Each run deploys the webapp at one replica with its HPA, warms up for 30 s at 5 RPS, then runs the profile: at `--scale 0.5`, 5 RPS for 180 s, 75 RPS for 120 s, 5 RPS again. It exports the Prometheus series, HPA events and pod timeline, then cools down for 15 s.

## Other runs

```sh
uv run run.py gpu --dry-run                                  # the plan
uv run run.py web --prefix lead --steps 2,4,8,12 --shape ramp
uv run run.py web --prefix duo --profile burst --scale 0.5 --scaler croft   # croft scale next to the HPA
uv run run.py calibrate                                      # CPU per request on each node
```

Only one experiment runs at a time (`data/experiment.lock`): two workloads sharing CPUs pollute each other's numbers.

## Applications and profiles

- `apps.json` lists the applications under test: namespace, Deployment, manifests, host, paths and the calibration `croft scale` needs. `--app` picks one; a new app is a new entry, not new code.
- `experiment/profiles.py` holds the open-loop profiles: constant, ramp, burst, periodic, variable.

## Output

```text
data/
├── gpu/runs.jsonl            one line per run: label, params, node, container_id, start, end
└── web/
    ├── runs.jsonl            one line per run: load, replicas or hpa, window, containers
    └── <run>/
        ├── workload.csv      open-loop runs: target and achieved RPS, latency per second
        ├── locust_*.csv      Locust runs
        ├── prometheus.csv    CPU, throttling, memory, network, restarts, replicas
        ├── scaler.log        --scaler croft: the scaler's decisions
        └── timeline.json     rescales, replica set changes, pod created and Ready times
```

The analysis notebooks are in `research/`: `burst_signals.ipynb` and `lead_time.ipynb`.
