# Running the web experiments

This guide runs burst experiments against the two web applications in `apps.json`, `webapp` and `hotel-reservation`, and compares three autoscalers on them: the Kubernetes HPA, KEDA and croft.

Every run deploys the application fresh, offers it an open-loop burst of requests, and records what the client, Prometheus and the autoscaler saw over the same window. [README.md](README.md) explains what is recorded and why.

**Table of contents**

- [Prerequisites](#prerequisites)
- [Applications](#applications)
  - [webapp](#webapp)
  - [hotel-reservation](#hotel-reservation)
- [Setting up KEDA](#setting-up-keda)
- [Running an experiment](#running-an-experiment)
  - [A single run](#a-single-run)
  - [Choosing the load](#choosing-the-load)
  - [Comparing autoscalers](#comparing-autoscalers)
- [Scoring runs](#scoring-runs)
- [Validating a run](#validating-a-run)
- [Reference run](#reference-run)
- [Calibration](#calibration)
- [Troubleshooting](#troubleshooting)
- [Cleaning up](#cleaning-up)

## Prerequisites

- The k3s cluster from [`labs/k3s-cluster`](../../k3s-cluster), with ingress-nginx on NodePort 30080 and kube-prometheus-stack.
- The croft collector DaemonSet with `CROFT_METRICS_ADDR` set, for the croft arm.
- KEDA, for the KEDA arm. See [Setting up KEDA](#setting-up-keda).
- [`uv`](https://docs.astral.sh/uv/) and `kubectl` on the server, `node-a`.

Run the harness on the server. `apps.json` reaches the applications at `127.0.0.1`, and the load generator should not share a network path with anything else:

```bash
ssh <user>@<server>
cd ~/daniel/github/cloudx/croft/labs/research/experiment
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml
kubectl get nodes        # every node Ready
```

> **Note**
> A laptop on the tailnet can follow a run with `~/.kube/croft-k3s-tailscale.yaml` and Grafana or Prometheus at `*.100.64.0.10.sslip.io:30080`.

## Applications

Both applications sit behind the same ingress and take the same load profiles. They differ by about fifty times in CPU per request, so each has its own load scale.

| Application | Shape | CPU per request | Pod CPU limit | `--scale` |
|---|---|---:|---:|---:|
| `webapp` | one Python service | 40 to 65 ms | 1 core | 0.5 |
| `hotel-reservation` | DeathStarBench, 8 Go services, MongoDB, memcached | 1.6 ms at the frontend | 200m | 2 |

### webapp

A CPU-bound HTTP service built from `workloads/webapp`. Its image is in no registry.

1. Build the image and load it on every node:
   ```bash
   docker build -t croft-webapp:v1 workloads/webapp
   (cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 --ask-become-pass)
   ```
2. Deploy it once and check that it answers:
   ```bash
   kubectl apply -f workloads/webapp/k8s.yaml
   kubectl -n croft-exp rollout status deployment/webapp
   curl -i http://web.127.0.0.1.sslip.io:30080/cpu
   ```

### hotel-reservation

[DeathStarBench](https://github.com/delimitrou/DeathStarBench) hotelReservation. Every request is a hotel search: the frontend calls `search`, which calls `geo` and `rate`, then `reservation` and `profile`. The HPA scales the frontend; the other services stay at one replica.

1. Pull the 1.2 GB service image on every node:
   ```bash
   docker pull deathstarbench/hotel-reservation:0.3.5
   (cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=deathstarbench/hotel-reservation:0.3.5 --ask-become-pass)
   ```
2. Deploy it once and search:
   ```bash
   kubectl apply -f workloads/hotel-reservation/k8s.yaml
   kubectl -n hotel-res wait --for=condition=Available deployment --all --timeout=600s
   curl -i "http://hotel.127.0.0.1.sslip.io:30080/hotels?inDate=2015-04-09&outDate=2015-04-10&lat=38.0235&lon=-122.095"
   ```
   A search returns a GeoJSON `FeatureCollection` of hotels.

> **Note**
> The Go services restart once or twice on the first deploy while MongoDB and consul start. A service still restarting after two minutes is a fault.

> **Note**
> MongoDB runs on `emptyDir`; each service seeds its collection at startup. If a MongoDB pod restarts, restart the services too: `kubectl -n hotel-res rollout restart deployment`.

## Setting up KEDA

KEDA scales on the request rate the ingress reports through Prometheus. Set it up once per cluster:

1. Let ingress-nginx export per-ingress metrics on port 10254. The controller restarts for a few seconds:
   ```bash
   kubectl -n ingress-nginx patch deployment ingress-nginx-controller --type=json -p '[
     {"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--enable-metrics=true"},
     {"op":"add","path":"/spec/template/spec/containers/0/ports/-","value":{"name":"metrics","containerPort":10254,"protocol":"TCP"}}]'
   ```
2. Scrape them every 5 s instead of the stack's 30 s, so KEDA is not late by configuration alone:
   ```bash
   kubectl apply -f workloads/ingress-nginx/podmonitor.yaml
   ```
3. Install KEDA:
   ```bash
   kubectl apply --server-side -f https://github.com/kedacore/keda/releases/download/v2.21.0/keda-2.21.0.yaml
   kubectl -n keda rollout status deployment
   ```
4. Check that Prometheus has the request counts:
   ```bash
   curl -s http://prometheus.127.0.0.1.sslip.io:30080/api/v1/query \
     --data-urlencode 'query=sum by (service) (rate(nginx_ingress_controller_requests[30s]))'
   ```

## Running an experiment

### A single run

```bash
uv run run.py web --app hotel-reservation --prefix hr-try --profile burst --scale 2 --probes relaxed
```

A run takes about 12 minutes and goes through five phases:

1. **Deploy.** The Deployment is applied, scaled to one replica and restarted; the previous run's HPA or ScaledObject is removed and this run's autoscaler applied.
2. **Warm-up.** 30 s at 5 RPS, not recorded.
3. **Measurement.** The profile runs open loop: requests leave at the target rate whether or not earlier ones were answered.
4. **Export.** Prometheus series, HPA and pod events, and the run line in `data/web/runs.jsonl`.
5. **Cool-down.** 15 s.

The result lands in `data/web/<prefix>-<profile>/`:

| File | Content |
|---|---|
| `workload.csv` | target and achieved RPS, successes, failures, mean and p95 latency, per second |
| `prometheus.csv` | CPU, throttled share, memory, network, restarts and replicas, every 5 s |
| `timeline.json` | HPA rescales, replica set changes, pod created and Ready times |
| `scaler.log` | croft arm only: croft's estimate every poll and its decisions |

`--probes relaxed` keeps an overloaded pod alive and in its Service. Use it for every comparison, so a restart does not hide the burst.

> **Note**
> One experiment holds the cluster at a time. A second `run.py` started meanwhile exits with `another experiment holds …`.

### Choosing the load

| Profile | Rates at `--scale 1` | Duration | Use |
|---|---|---|---|
| `constant` | 25 RPS | 30 s | check that the setup works |
| `ramp` | 10 to 200 RPS | 600 s | find one pod's capacity |
| `burst` | 10 RPS, 150 RPS from 180 s to 300 s, then 10 RPS | 600 s | reaction and recovery |
| `periodic` | 10 to 100 RPS, period 120 s | 600 s | autoscaling lag |
| `variable` | 10 to 100 RPS, a new level every 30 s, seed 42 | 600 s | prediction |

`--scale` multiplies every rate. A good burst saturates one pod at least twice over and stays within ten pods' worth:

- `webapp` at `--scale 0.5`: 5, then 75, then 5 RPS. 75 RPS needs 6 to 7 pods at 70%.
- `hotel-reservation` at `--scale 2`: 20, then 300, then 20 RPS. One frontend pod holds about 130 RPS at 70%.

For a new application or a changed manifest, find one pod's capacity first with a ramp at fixed replicas, and read the RPS at which p95 leaves its floor:

```bash
uv run run.py web --app hotel-reservation --prefix hr-ramp --profile ramp --scale 2 --replicas 1
```

### Comparing autoscalers

`--scaler` picks the arm:

| Arm | Who adds replicas | Signal | Signal path |
|---|---|---|---|
| `none` | the HPA from `hpa.yaml` | CPU at 70% of the request | metrics-server, checked every 15 s |
| `keda` | the HPA KEDA builds from a ScaledObject | `nginx_ingress_controller_requests` for the app's Service | Prometheus every 5 s, `rate()` over 30 s, then the HPA |
| `croft` | `croft scale`, next to the HPA | the calibrated syscall rate from eBPF | collector `/metrics` every 2 s |

The three arms share one capacity model: a pod is full at 70% of its CPU limit. KEDA's threshold is that capacity in requests per second, `0.7 × CPU limit ÷ CPU per request`: 14 RPS per pod for `webapp`, 87.5 for `hotel-reservation`. The harness computes it from `apps.json` and the deployed limit. The arms therefore differ only in their signal and how late it arrives.

Alternate the arms within each repetition, so drift over the afternoon falls on all three:

```bash
for i in 1 2 3; do
  uv run run.py web --app hotel-reservation --prefix hr-hpa-$i  --profile burst --scale 2 --probes relaxed || break
  uv run run.py web --app hotel-reservation --prefix hr-keda-$i --profile burst --scale 2 --probes relaxed --scaler keda || break
  uv run run.py web --app hotel-reservation --prefix hr-duo-$i  --profile burst --scale 2 --probes relaxed --scaler croft || break
done
```

The full study is both applications times three arms times three repetitions: 18 runs, 3.5 to 4 hours of cluster time. Compare reaction times, SLO seconds and replica-seconds between arms within one application; absolute latencies differ between the applications by design.

> **Note**
> nginx counts a request when it completes, but the ingress still sees the offered load sooner than a saturated pod's syscalls do, which only show what the pod manages to answer ([#8](https://github.com/danielcristho/croft/issues/8)). Expect KEDA to be the stronger baseline.

## Scoring runs

```bash
uv run run.py compare --prefix hr-hpa- hr-keda- hr-duo-
```

`compare` groups runs by app, profile, scale, probes and arm, prints mean ± sd for each group, and writes one row per run to `data/web/compare.csv`:

| Column | Meaning |
|---|---|
| `hpa_first_up_s` | burst start to the first HPA scale-up; in the KEDA arm, the HPA KEDA built |
| `croft_first_up_s` | burst start to croft's first scale-up |
| `first_ready_s` | burst start to the first new pod Ready |
| `peak_replicas`, `time_to_peak_s` | the most replicas, and when |
| `scale_down_events` | scale-downs inside the burst, such as the HPA undoing croft |
| `slo_violation_s` | seconds with p95 above 500 ms |
| `failed_fraction` | failed over answered requests in the burst |
| `success_ratio` | successes over offered requests in the burst |
| `replica_seconds` | replicas times seconds over the run, the cost |

`[k]` after a value means only k runs of the group have it.

## Validating a run

A finished command is not an accepted run. Check:

- `achieved_rps` follows `target_rps` outside the burst, with no failures at low load.
- The burst saturated one pod before the autoscaler reacted: p95 rose or `cpu_throttled_fraction` climbed.
- The autoscaler scaled up and stayed within 10 replicas.
- No pod restarted (`restarts` in `prometheus.csv`).
- Prometheus answered for the whole window: no empty columns.
- Nothing else ran on the cluster: no other experiment, build or GPU job.
- For `hotel-reservation`, the backend services stayed under their 1-core limits (`kubectl top pods -n hotel-res`). A saturated `rate` or `search` moves the bottleneck away from the frontend.

Leave rejected runs out of `compare` by choosing prefixes that skip them. Never edit a finished run directory; change the setup and run again under a new prefix.

## Reference run

`hotel-reservation`, `--scale 2`, `--probes relaxed`, HPA alone:

| | Before the burst | Burst | After |
|---|---:|---:|---:|
| Target / achieved RPS | 20 / 20 | 300 / 297 | 20 / 21 |
| Median p95 latency | 12 ms | 1,181 ms | 13 ms |
| Seconds with p95 above 500 ms | 1 | 72 | 0 |
| Frontend CPU throttled | 0% | 44 to 92% | 0% |

The HPA scaled up 35 s into the burst and reached 3 replicas at 114 s. No request failed and no pod restarted. A run far from this points at the setup, not the autoscaler.

## Calibration

The croft and KEDA arms need each application's `scaler` entry in `apps.json`:

- `syscall`: the syscall counted per request.
- `calls_per_request`: calls of it per request.
- `cpu_per_request_ms`: the CPU one request costs.

| Application | `syscall` | `calls_per_request` | `cpu_per_request_ms` |
|---|---|---:|---:|
| `webapp` | `sendto` | 2.06 | 50 |
| `hotel-reservation` | `write` | 11.4 | 1.6 |

The hotel frontend is Go, which answers through `write` rather than `sendto`. Its values come from one frontend pod without an autoscaler, at 20, 50 and 100 RPS for 75 s each, read from the collector's `/metrics`. `write` per request moved 1% across the levels; CPU per request went from 1.8 to 1.55 ms.

Recalibrate after changing an application's image or its frontend resources.

## Troubleshooting

| Message | Cause and fix |
|---|---|
| `<image> is missing on <nodes>` | Load the image on those nodes, see [Applications](#applications). The node status can take a few minutes to list it. |
| `another experiment holds data/experiment.lock` | A run is in progress. Wait for it, or stop it. |
| `KEDA is not installed` | Follow [Setting up KEDA](#setting-up-keda). |
| `apps.json has no calibration for <app>` | Measure one, see [Calibration](#calibration). |
| `kubectl get nodes … returned non-zero exit status` | `KUBECONFIG` is not set in this shell. |
| `<host>/ did not answer 200 within 120 s` | The ingress has no Ready backend. Check `kubectl get pods -n <namespace>`. |

Long series outlive an SSH session; run them under `tmux` or `nohup`.

## Cleaning up

Both applications can stay deployed; an idle one uses little CPU. To remove them:

```bash
kubectl delete namespace hotel-res
kubectl delete -f workloads/webapp/k8s.yaml
```

After a series, pull croft's datasets for the same window:

```bash
(cd ../../k3s-cluster && ansible-playbook collect.yml)
```
