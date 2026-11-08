# Experiment setup

The configuration behind the comparison series and the service-interaction study. Each run also records its own values in `data/web/runs.jsonl`.

## 1. Load generator

Runs with `--profile` use the open-loop generator in `experiment/rps.py`, not Locust. Both studies use `--profile`.

| Setting | Value |
|---|---|
| Model | open-loop: requests leave at the target rate whether or not earlier ones answered |
| Implementation | one `asyncio` loop, `GENERATOR = 2`, recorded as `generator` in `runs.jsonl` |
| Concurrent requests | not fixed and not capped; `in_flight` in `workload.csv` records them |
| Timeout | 5 s per request, from its scheduled send |
| Success | HTTP 2xx or 3xx; redirects are not followed |
| Connections | a new one per request |
| DNS | the host is resolved once, at the start of the load |
| Rows in `workload.csv` | one per second, counting requests when they complete |
| Generator location | one process on the control-plane node |
| Path to the app | loopback, then ingress-nginx on NodePort 30080 |

> **Note**
> - Latency starts at the scheduled send and ends at the first byte of the body, not the whole response.
> - A late send does not catch up; the schedule restarts from that moment.
> - `failed` and latency belong to the second a request completes, so a per-second success rate can exceed 100%. Totals over a run are unaffected.

### Earlier generator

Runs without a `generator` field in `runs.jsonl` used an earlier version, so their latency cannot be compared directly with later runs:

- at most 256 requests in flight; the rest queued
- latency started when a worker took the request, so time in the queue was not counted
- 5 s timeout per socket operation, not covering DNS; requests were recorded up to about 40 s
- the host was resolved for every request

Locust remains for the `--users` and `--steps` modes (`load/locustfile.py`: 10 to 50 ms wait, 5 users/s spawn rate). Neither study uses them.

## 2. Load profiles

Every rate is multiplied by `--scale`. The warm-up rate is not.

| Profile | Duration | Rate at scale 1 | Rise |
|---|---|---|---|
| `burst` | 900 s | 10 RPS, then 150 RPS from 180 s to 300 s, then 10 RPS | immediate, no ramp-up |
| `ramp` | 600 s | 10 RPS rising to 200 RPS | linear over the whole run |
| `constant` | 30 s | 25 RPS | none |

Each run follows the same order:

1. Deploy one fresh frontend replica and a new HPA.
2. Warm up at 5 RPS for 30 s.
3. Run the profile.
4. Export the series, then cool down for 15 s.

Each application offers one URL:

| Application | Path | Default scale |
|---|---|---:|
| `hotel-reservation` | `/hotels?inDate=2015-04-09&outDate=2015-04-10&lat=38.0235&lon=-122.095` | 2 |
| `online-boutique` | `/` | 1 |
| `otel-demo` | `/api/recommendations?productIds=OLJCESPC7Z&currencyCode=EUR` | 1 |

## 3. Studies

| | Comparison series | Service-interaction study |
|---|---|---|
| Arms | HPA, `croft scale` | HPA |
| Profiles | `burst` | `burst`, `ramp` |
| Scale | the application's default | `lo`, `hi` |
| Repetitions | 3 | 3 |
| Runs | 18 | 36 |
| Linkerd | no | yes |
| Probes | `relaxed` | `relaxed` |

### Scales of the first interaction series

Commit `789540f`, 2026-11-06 and 2026-11-07. These scales were not derived from a capacity measurement, so this series does not follow the rule in [RUNNING.md section 18](RUNNING.md#18-study-service-interactions). It used the earlier generator. The series is kept under `si-*` and will be run again under a new series name.

| Application | Scale | `burst` base and peak (RPS) | `ramp` (RPS) |
|---|---:|---:|---:|
| `hotel-reservation` | 1.33 | 13.3, 199.5 | 13.3 to 266 |
| `hotel-reservation` | 2.67 | 26.7, 400.5 | 26.7 to 534 |
| `online-boutique` | 0.80 | 8, 120 | 8 to 160 |
| `online-boutique` | 1.60 | 16, 240 | 16 to 320 |
| `otel-demo` | 0.53 | 5.3, 79.5 | 5.3 to 106 |
| `otel-demo` | 1.07 | 10.7, 160.5 | 10.7 to 214 |

`otel-demo` in that series ran with `astronomy-db` at 80Mi, and 78 to 100% of its requests failed. The limit is now 512Mi.

## 4. Autoscaling

Both arms start at one replica and stop at ten.

| Setting | HPA | `croft scale` |
|---|---|---|
| Signal | CPU of the frontend container (`ContainerResource`) | syscall rate of the frontend, from the collector |
| Target | 70% of the CPU request | 70% (`--target 0.7`) |
| Decision interval | Kubernetes default | 2 s |
| Confirmation | none | 2 polls |
| Scale-down | Kubernetes default | 300 s window |
| Metric source | metrics-server, 15 s resolution | collector, polled every 2 s |

`hpa.yaml` sets no `behavior`, so the HPA uses the Kubernetes defaults.

### Delay before a decision

| Step | HPA | `croft scale` |
|---|---|---|
| Measurement | kubelet averages CPU over about 15 s | collector reads eBPF counters every 2 s |
| Collection | metrics-server, `--metric-resolution=15s` | `croft scale` reads each collector's `/metrics` every 2 s, not through Prometheus |
| Decision | controller loop, 15 s by default | after 2 polls that agree, at least 4 s |
| Scale-up stabilization | 0 s by default | none |

Checked on the cluster on 2026-11-08: metrics-server runs `--metric-resolution=15s`, and the frontend's samples covered 11 to 18 s windows and were 14 to 24 s old. The k3s server sets no HPA sync flag on its command line; `/etc/rancher/k3s/config.yaml` was not read, so the 15 s loop is the default, not a confirmed value. The HPA keeps these defaults because the comparison is with a stock HPA.

The 2 s poll dates from the collector's first version and was not tuned. eBPF keeps the counts in the kernel, so a short poll costs a map read, not a scrape of every pod. The collector also writes its dataset every 2 s, so a shorter `croft scale` poll would read the same values again.

`apps.json` holds the `croft scale` calibration:

| Application | Syscall | Calls per request | CPU per request (ms) |
|---|---|---:|---:|
| `hotel-reservation` | `write` | 11.4 | 1.6 |
| `online-boutique` | `getpeername` | 2.0 | 3.26 |
| `otel-demo` | `writev` | 5.32 | 5.64 |

## 5. Workloads

CPU and memory of the frontend, the deployment under test:

| Application | Image | Container | CPU | Memory |
|---|---|---|---:|---:|
| `hotel-reservation` | `deathstarbench/hotel-reservation:0.3.5` | `frontend` | 200m | 256Mi |
| `online-boutique` | `microservices-demo/frontend:v0.10.7` | `server` | 200m | 256Mi |
| `otel-demo` | `open-telemetry/demo:3.1.0-frontend` | `frontend` | 300m | 512Mi |

Requests equal limits. The other services are in `workloads/<application>/k8s.yaml`.

`--probes relaxed` removes the liveness probe and sets the readiness probe to a 10 s timeout and 6 failures.

The frontend is pinned to one worker node with `--node`. The scheduler places the other services, so backend placement can change when an application restarts. A run records only the frontend's node.

## 6. Cluster

k3s v1.36.4 with containerd 2.3.4.

| Role | CPU | Memory | Kernel | OS |
|---|---:|---:|---|---|
| control plane, load generator, ingress, Prometheus | 24 | 62 GiB | 7.0.0-42 | Ubuntu 24.04 |
| worker, frontend under test | 20 | 31 GiB | 7.0.0-42 | Ubuntu 24.04 |
| worker | 24 | 14 GiB | 7.0.0-30 | Ubuntu 26.04 |

During the first interaction series the control plane used about 2.5 to 3.4 cores and the second worker about 4 to 7 cores, measured every 30 minutes. The source of the second worker's usage was not identified.

## 7. Measurement

| Component | Version | Interval |
|---|---|---|
| Prometheus | v3.15.0 | 30 s by default |
| Collector | `croft-collector` | polled every 2 s, scraped every 5 s |
| Linkerd proxies | edge-26.9.3 | scraped every 10 s |
| ingress-nginx | v1.15.1 | |

`prometheus.csv` has a 5 s step with `rate()` over 1 minute. `edges.csv` and `services.csv` use a 30 s window.

`signals.csv` holds the collector's series for every service, in long form (`unix_s`, `service`, `signal`, `key`, `value`), with a 5 s step, `rate()` over 15 s for counters and without the Linkerd proxy. A missing row means zero. Runs before this file existed can be exported while Prometheus keeps them, 10 days.

The collector image running on the cluster is `croft-collector:operands`, while the manifests name `croft-collector:dev`.
