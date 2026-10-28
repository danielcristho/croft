---
title: Experiments
description: Run a burst against a web service, with the HPA as the baseline.
sidebar:
  order: 5
---

The harness in `labs/research/experiment` runs controlled workloads on the [lab cluster](/croft/guides/lab-cluster/) and records them so croft's dataset joins them exactly: every row carries the container id, so a run's rows are found by id, not by time.

The question behind the web experiments:

> Can an autoscaler use eBPF telemetry to detect a workload burst early enough to add replicas before the application degrades?

Each run changes the offered load and records, for the same window, what the client, the kernel (through croft, every 2 s), the HPA (every 15 s) and Prometheus saw.

The step-by-step procedure for the web applications, `webapp` and `hotel-reservation`, and the comparison of the Kubernetes HPA, KEDA and croft is in [`RUNNING.md`](https://github.com/danielcristho/croft/blob/main/labs/research/experiment/RUNNING.md).

## Suites

| Suite | Workloads |
|---|---|
| `gpu` | training, batch inference, generation, HTTP and ollama serving, an idle GPU; each a Job asking for one GPU |
| `web` | an application of `apps.json` (`webapp` or `hotel-reservation`) under Locust users or open-loop RPS, at fixed replicas or under an autoscaler |

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
uv run run.py compare --prefix hpa- duo-                     # burst runs scored per arm
```

Only one experiment runs at a time (`data/experiment.lock`): two workloads sharing CPUs pollute each other's numbers.

## Kubernetes, KEDA and croft

`--scaler` picks the autoscaler of a burst run: `none` for the HPA alone, `keda` for a KEDA ScaledObject on the ingress request rate, `croft` for croft scale next to the HPA. All three assume the same capacity per pod, so they differ only in their signal.

```sh
for i in 1 2 3; do
  uv run run.py web --app hotel-reservation --prefix hr-hpa-$i  --profile burst --scale 2 --probes relaxed || break
  uv run run.py web --app hotel-reservation --prefix hr-keda-$i --profile burst --scale 2 --probes relaxed --scaler keda || break
  uv run run.py web --app hotel-reservation --prefix hr-duo-$i  --profile burst --scale 2 --probes relaxed --scaler croft || break
done
uv run run.py compare --prefix hr-hpa- hr-keda- hr-duo-
```

`compare` scores each burst run (first scale-up, first new pod Ready, peak replicas, scale-downs inside the burst, seconds above the 500 ms p95 objective, failures, replica-seconds) and prints mean ± sd per arm; the per-run rows go to `data/web/compare.csv`.

## Applications and profiles

- `apps.json` lists the applications under test: namespace, Deployment, manifests, host, paths and the calibration croft scale and KEDA need. `--app` picks one; a new app is a new entry, not new code.
- `experiment/profiles.py` holds the open-loop profiles: constant, ramp, burst, periodic, variable.

## Output

| Path under `data/` | Content |
|---|---|
| `gpu/runs.jsonl` | one line per GPU run: label, parameters, node, container id, start, end |
| `web/runs.jsonl` | one line per web run: app, load, autoscaler, window, containers |
| `web/compare.csv` | `run.py compare`: one scored row per burst run |
| `web/<run>/workload.csv` | open-loop runs: target and achieved RPS, latency per second |
| `web/<run>/locust_*.csv` | Locust runs |
| `web/<run>/prometheus.csv` | CPU, throttled share, memory, network, restarts, replicas |
| `web/<run>/timeline.json` | rescales, replica set changes, pod created and Ready times |
| `web/<run>/scaler.log` | `--scaler croft`: croft's estimate every poll and its decisions |

The analysis notebooks are in `research/`: `burst_signals.ipynb` and `lead_time.ipynb`.
