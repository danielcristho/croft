# croft Experiments — Comparing HPA, KEDA and croft

This guide is the step-by-step procedure for the **burst autoscaling experiments**.

It compares three autoscalers on the same applications, under the same load, on the same node:

- the Kubernetes **HPA**, scaling on CPU from metrics-server;
- **KEDA**, scaling on the request rate the ingress reports to Prometheus;
- **croft**, scaling on the syscall rate the collector reads through eBPF.

The harness itself, its suites and its output are described in [README.md](README.md). Deploying the applications is described in [workloads/README.md](workloads/README.md).

---

# 1. Purpose

The experiments ask one question:

> Can an autoscaler that reads eBPF telemetry see a burst early enough to add replicas before the application degrades?

Every run deploys the application fresh, offers it an open-loop burst of requests, and records what the client, Prometheus and the autoscaler saw over the same window.

The research flow is:

```text
application under test
         ^
         |
open-loop burst  ---->  workload.csv
         |
         +---- HPA / KEDA / croft  ---->  timeline.json, scaler.log
         |
         +---- Prometheus export   ---->  prometheus.csv
                       |
                       v
               run.py compare
                       |
                       v
                  compare.csv
```

---

# 2. The three arms

Each arm is one value of `--scaler`.

| Arm | Who adds replicas | Signal | Signal path |
|---|---|---|---|
| `none` | the HPA from `hpa.yaml` | CPU at 70% of the request | metrics-server, checked every 15 s |
| `keda` | the HPA that KEDA builds from a ScaledObject | `nginx_ingress_controller_requests` for the app's Service | Prometheus every 5 s, `rate()` over 30 s, then the HPA |
| `croft` | `croft scale`, next to the HPA | the calibrated syscall rate | collector `/metrics` every 2 s |

The three arms share one capacity model:

```text
a pod is full at 70% of its CPU limit
```

KEDA's threshold is that capacity in requests per second:

```text
threshold = 0.7 × CPU limit ÷ CPU per request
```

The harness computes it from `apps.json` and the deployed limit.

The arms therefore differ only in **their signal** and **how late it arrives**.

---

# 3. Prerequisites

The cluster must provide:

```text
k3s cluster from labs/k3s-cluster
ingress-nginx on NodePort 30080
kube-prometheus-stack
croft collector DaemonSet with CROFT_METRICS_ADDR set   (croft arm)
KEDA                                                    (KEDA arm)
```

The machine running the harness needs:

```text
uv
kubectl
just
```

Run the harness on the server, `node-a`.

`apps.json` reaches the applications at `127.0.0.1`, and the load generator should not share a network path with anything else.

---

# 4. Enter the harness

Log in to the server:

```sh
ssh <user>@<server>
cd ~/daniel/github/cloudx/croft/labs/research/experiment
```

Install the Python environment:

```sh
uv sync
```

Point `kubectl` at the cluster:

```sh
export KUBECONFIG=~/.kube/croft-k3s.yaml
```

The `just` recipes set this default on their own.

---

# 5. Verify the cluster

Check the nodes:

```sh
kubectl get nodes
```

Every node must be `Ready`.

Check that metrics-server answers:

```sh
kubectl top nodes
```

Check that the collector runs on every node:

```sh
kubectl -n croft get pods -l app=croft-collector -o wide
```

Do not run experiments until all three checks pass.

A laptop on the tailnet can follow a run with `~/.kube/croft-k3s-tailscale.yaml`, and open Grafana or Prometheus at `*.100.64.0.10.sslip.io:30080`.

---

# 6. Prepare the applications

Three applications are available:

| Application | Shape | CPU per request | Pod CPU limit | `--scale` |
|---|---|---:|---:|---:|
| `hotel-reservation` | DeathStarBench, 8 Go services, MongoDB, memcached | 1.6 ms at the frontend | 200m | 2 |
| `online-boutique` | Online Boutique, a Go frontend over 10 services, Redis | measured in section 8 | 200m | set in section 11 |
| `otel-demo` | OpenTelemetry Demo, Node.js, Python, Go and C++ services, PostgreSQL | 5.6 ms at the frontend | 300m | 1 |

Each autoscaler scales the application's frontend. The services behind it stay at one replica.

Deploy each application once by hand, before its first run, as [workloads/README.md](workloads/README.md) describes.

For example:

```sh
just samples deploy hotel-reservation
just samples smoke hotel-reservation
```

A successful smoke test resembles:

```text
200 0.014s
```

Do not start a run until the smoke test returns `200`.

---

# 7. Set up KEDA

KEDA scales on the request rate that the ingress reports through Prometheus.

Set it up once per cluster.

Let ingress-nginx export per-ingress metrics on port 10254:

```sh
kubectl -n ingress-nginx patch deployment ingress-nginx-controller --type=json -p '[
  {"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--enable-metrics=true"},
  {"op":"add","path":"/spec/template/spec/containers/0/ports/-","value":{"name":"metrics","containerPort":10254,"protocol":"TCP"}}]'
```

The controller restarts for a few seconds.

Scrape the metrics every 5 s instead of the stack's 30 s, so KEDA is not late by configuration alone:

```sh
kubectl apply -f workloads/ingress-nginx/podmonitor.yaml
```

Install KEDA:

```sh
kubectl apply --server-side -f https://github.com/kedacore/keda/releases/download/v2.21.0/keda-2.21.0.yaml
kubectl -n keda rollout status deployment
```

Check that Prometheus has the request counts:

```sh
curl -s http://prometheus.127.0.0.1.sslip.io:30080/api/v1/query \
  --data-urlencode 'query=sum by (service) (rate(nginx_ingress_controller_requests[30s]))'
```

The answer must list a `service` for each application that received traffic.

---

# 8. Calibrate an application

The croft and KEDA arms need each application's `scaler` entry in `apps.json`:

```text
syscall              the syscall counted per request
calls_per_request    calls of it per request
cpu_per_request_ms   the CPU one request costs
```

Measure it with:

```sh
just samples calibrate otel-demo
```

The default levels are 20, 50 and 100 RPS, 75 s each. A calibration takes about 5 minutes.

Other levels:

```sh
just samples calibrate otel-demo "10 20 35"
```

The recipe:

```text
deploys the application at one replica, pinned to node-a, without an autoscaler
offers each level as constant load
reads the pod's syscall rates and CPU from the collector on its node
reads the busiest pods of the namespace from metrics-server
```

A successful calibration resembles:

```text
otel-demo: frontend-66f497f847-4mbx2 on node-a, CPU limit 1
    20 RPS: achieved 20.0, failed 0, p95 17 ms, CPU 6.26 ms/request, throttled 0%
        busiest pods: frontend 140m, recommendation 75m, astronomy-db 61m, product-catalog 49m
    50 RPS: achieved 50.0, failed 0, p95 13 ms, CPU 4.15 ms/request, throttled 0%
        busiest pods: frontend 209m, recommendation 164m, product-catalog 160m, astronomy-db 140m
   100 RPS: achieved 100.0, failed 0, p95 58 ms, CPU 3.80 ms/request, throttled 0%
        busiest pods: recommendation 292m, astronomy-db 218m, frontend 127m, product-catalog 79m
syscalls per request, least spread first: {'writev': 0.062, 'getpid': 0.126, 'read': 0.25, ...}
apps.json scaler: {"syscall": "writev", "calls_per_request": 5.21, "cpu_per_request_ms": 4.73}
one pod at 70% of 1 core holds about 148 RPS; details in .../data/calibration/otel-demo.json
```

Copy the `apps.json scaler` line into the application's entry.

The current values:

| Application | `syscall` | `calls_per_request` | `cpu_per_request_ms` |
|---|---|---:|---:|
| `hotel-reservation` | `write` | 11.4 | 1.6 |
| `online-boutique` | not yet measured | | |
| `otel-demo` | `writev` | 5.32 | 5.64 |

The Go and Node.js frontends answer through `write` and `writev`. Take the syscall the calibration picks, not the one another application uses.

The `otel-demo` values were measured at its 300m limit, at 10, 20 and 35 RPS. `writev` per request moved 3% across the levels; CPU per request fell from 7.1 to 4.4 ms, because the Node.js frontend spends about 40m whatever the load.

---

# 9. Validate a calibration

Accept a calibration only when, at every level:

```text
[ ] achieved RPS equals the target
[ ] failed is 0
[ ] p95 stays flat
[ ] throttled is 0%
[ ] no other pod of the namespace nears its own CPU limit
```

Otherwise the levels are too high, or a backend service saturates before the frontend.

The Node.js frontend of `otel-demo` is the exception to the throttling rule. It spends its CPU in short bursts, so a 300m limit throttles 6 to 10% of CFS periods well below saturation, at 20 and 35 RPS. Accept it while p95 stays far below 500 ms.

Choose lower levels and run it again.

Recalibrate after changing an application's image or its frontend resources.

---

# 10. Understand the run phases

Every run consists of:

```text
time ------------------------------------------------------------------->

  deploy        warm-up          measurement          export     cool-down
|--------|-----------------|----------------------|-----------|-----------|
 fresh pods   30 s at 5 RPS     the RPS profile      Prometheus     15 s
```

## Deploy

The Deployment is applied, scaled to one replica and restarted.

The previous run's HPA or ScaledObject is removed, and this run's autoscaler applied.

## Warm-up

30 s at 5 RPS. Not recorded.

## Measurement

The profile runs open loop: requests leave at the target rate whether or not earlier ones were answered.

## Export

Prometheus series, HPA and pod events, and the run line in `data/web/runs.jsonl`.

## Cool-down

15 s. It separates consecutive runs.

A burst run takes about 12 minutes.

---

# 11. Choose the load

Five profiles are available:

| Profile | Rates at `--scale 1` | Duration | Use |
|---|---|---|---|
| `constant` | 25 RPS | 30 s | check that the setup works |
| `ramp` | 10 to 200 RPS | 600 s | find one pod's capacity |
| `burst` | 10 RPS, 150 RPS from 180 s to 300 s, then 10 RPS | 600 s | reaction and recovery |
| `periodic` | 10 to 100 RPS, period 120 s | 600 s | autoscaling lag |
| `variable` | 10 to 100 RPS, a new level every 30 s, seed 42 | 600 s | prediction |

`--scale` multiplies every rate. It defaults to the application's `scale` in `apps.json`.

A good burst saturates one pod at least twice over and stays within ten pods:

```text
hotel-reservation  --scale 2    20 → 300 → 20 RPS    87.5 RPS per pod  about 4 pods
otel-demo          --scale 1    10 → 150 → 10 RPS    37 RPS per pod    about 4 pods
online-boutique    set from its calibration, for about 4 pods
```

For a new application or a changed manifest, find one pod's capacity first with a ramp at a fixed replica count:

```sh
uv run run.py web --app hotel-reservation --prefix hr-ramp --profile ramp --scale 2 --replicas 1
```

Read the RPS at which p95 leaves its floor.

---

# 12. Run a first burst

Run each arm once on the application before a series:

```sh
just samples burst otel-demo none   otel-try
just samples burst otel-demo keda   otel-try
just samples burst otel-demo croft  otel-try
```

Each arm lands in its own directory, `data/web/otel-try-<arm>-burst/`.

The recipe expands to:

```sh
uv run run.py web --app otel-demo --node node-a --prefix otel-try-none \
  --profile burst --probes relaxed --scaler none
```

`--node node-a` pins every replica to one node.

The nodes differ by up to 1.69 times in CPU speed, and the scheduler otherwise places the first pod anywhere. In the smoke runs a single frontend on `node-a` held the 300 RPS hotel burst at a 143 ms median p95, while a pod on `node-b` reached 1,181 ms.

Without the pin, node placement outweighs the autoscaler.

`--probes relaxed` keeps an overloaded pod alive and in its Service, so a restart does not hide the burst.

Only one experiment holds the cluster at a time. A second run started meanwhile exits with:

```text
another experiment holds data/experiment.lock
```

---

# 13. Inspect the result

A run writes:

```text
data/web/<prefix>-<profile>/
├── workload.csv       target and achieved RPS, successes, failures, mean and p95 latency, per second
├── prometheus.csv     CPU, throttled share, memory, network, restarts and replicas, every 5 s
├── timeline.json      HPA rescales, replica set changes, pod created and Ready times
└── scaler.log         croft arm only: croft's estimate every poll and its decisions
```

Inspect the client side:

```sh
column -s, -t < data/web/otel-try-croft-burst/workload.csv | less -S
```

Inspect the replica changes:

```sh
python3 -m json.tool data/web/otel-try-croft-burst/timeline.json | less
```

## Live, in Grafana

The cluster's Grafana has a dashboard for a run: the arm, the ingress traffic and p95, the replicas, every frontend pod's lifetime, croft's syscall rate per pod, and CPU per pod.

Apply it once per cluster:

```sh
just samples monitoring
```

The recipe also lets Prometheus scrape the croft collector every 5 s, and prints the URL:

```text
http://grafana.127.0.0.1.sslip.io:30080/d/croft-experiment
http://grafana.100.64.0.10.sslip.io:30080/d/croft-experiment
```

Sign in as `admin` with the password from:

```sh
kubectl -n monitoring get secret monitoring-grafana -o jsonpath='{.data.admin-password}' | base64 -d; echo
```

Choose the application, and the syscall from its calibration: `write` for hotel-reservation, `writev` for otel-demo, and the one section 8 finds for online-boutique.

Set the time range to one run, from `started_ms` to `ended_ms` in `runs.jsonl`, to read the pods it created, its peak and its replica-seconds. The arm band at the top tells the runs of a series apart.

Replica counts come from kube-state-metrics, scraped every 30 s: a pod shows up to 30 s late. The ingress p95 counts the requests the client gave up on at 5 s, so it can read above 5 s. The scores come from the run's files, not from the dashboard.

---

# 14. Accept or reject a run

A finished command is not an accepted run.

Accept a run only when:

```text
[ ] achieved_rps follows target_rps outside the burst
[ ] no request failed at low load
[ ] the burst saturated one pod before the autoscaler reacted (p95 rose or cpu_throttled_fraction climbed)
[ ] the autoscaler scaled up and stayed within 10 replicas
[ ] no pod restarted (restarts in prometheus.csv)
[ ] Prometheus answered for the whole window (no empty columns)
[ ] the backend services stayed under their CPU limits
[ ] nothing else ran on the cluster
```

Reject a run when, for example:

```text
a backend service saturated and moved the bottleneck away from the frontend
another experiment, build or GPU job ran at the same time
the frontend pod landed on another node
Prometheus stopped scraping
```

Leave rejected runs out of `compare` by choosing prefixes that skip them.

Do not edit a finished run directory. Change the setup and run again under a new prefix.

---

# 15. Reference run

`hotel-reservation`, `--scale 2`, `--probes relaxed`, HPA alone:

| | Before the burst | Burst | After |
|---|---:|---:|---:|
| Target / achieved RPS | 20 / 20 | 300 / 297 | 20 / 21 |
| Median p95 latency | 12 ms | 1,181 ms | 13 ms |
| Seconds with p95 above 500 ms | 1 | 72 | 0 |
| Frontend CPU throttled | 0% | 44 to 92% | 0% |

The HPA scaled up 35 s into the burst and reached 3 replicas at 114 s.

No request failed and no pod restarted.

A run far from this points at the setup, not at the autoscaler.

---

# 16. Run the comparison series

A series runs the three arms three times, alternated, so drift over the afternoon falls on all three:

```sh
just samples series otel-demo otel
```

The recipe runs, in order:

```text
otel-none-1   otel-keda-1   otel-croft-1
otel-none-2   otel-keda-2   otel-croft-2
otel-none-3   otel-keda-3   otel-croft-3
```

and scores them at the end.

A series takes about 1 h 50 min per application.

It outlives an SSH session only under `tmux` or `nohup`:

```sh
tmux new -s series
just samples series hotel-reservation hr
just samples series online-boutique ob
just samples series otel-demo otel
```

The full study is 3 applications × 3 arms × 3 repetitions: 27 runs, about 5.5 hours of cluster time.

Compare the arms within one application. Absolute latencies differ between the applications by design.

nginx counts a request when it completes, but the ingress still sees the offered load sooner than a saturated pod's syscalls do, which only show what the pod manages to answer ([#8](https://github.com/danielcristho/croft/issues/8)). Expect KEDA to be the stronger baseline.

---

# 17. Score the runs

Score finished runs by prefix:

```sh
just samples compare otel-none- otel-keda- otel-croft-
```

`compare` groups the runs by app, profile, scale, probes and arm, prints mean ± sd for each group, and writes one row per run to `data/web/compare.csv`.

Rows of runs it scores again are replaced; rows of other runs stay, so scoring one series keeps the others.

| Column | Meaning |
|---|---|
| `hpa_first_up_s` | burst start to the first HPA scale-up; in the KEDA arm, the HPA KEDA built |
| `croft_first_up_s` | burst start to croft's first scale-up |
| `first_ready_s` | burst start to the first new pod Ready |
| `peak_replicas`, `time_to_peak_s` | the most replicas, and when |
| `scale_down_events` | scale-downs inside the burst, such as the HPA undoing croft |
| `pods_created` | pods the autoscaler added over the run: the sum of the replica increases |
| `slo_violation_s` | seconds with p95 above 500 ms |
| `failed_fraction` | failed over answered requests in the burst |
| `success_ratio` | successes over offered requests in the burst |
| `replica_seconds` | replicas times seconds over the run, the cost |

`[k]` after a value means only k runs of the group have it.

---

# 18. Troubleshooting

| Message | Cause and fix |
|---|---|
| `<image> is missing on <nodes>` | Load the image on those nodes, see [workloads/README.md](workloads/README.md). The node status can take a few minutes to list it. |
| `another experiment holds data/experiment.lock` | A run is in progress. Wait for it, or stop it. |
| `KEDA is not installed` | Follow [Set up KEDA](#7-set-up-keda). |
| `apps.json has no calibration for <app>` | Measure one, see [Calibrate an application](#8-calibrate-an-application). |
| KEDA's HPA shows `0/<threshold>` under load | The query matched no series. Prometheus moves the metric's `namespace` to `exported_namespace`; filter on that. |
| `kubectl get nodes … returned non-zero exit status` | `KUBECONFIG` is not set in this shell. |
| `<host>/ did not answer 200 within 120 s` | The ingress has no Ready backend. Check `just samples status <app>`. |

---

# 19. Reproducibility checklist

Every accepted run preserves:

```text
[ ] application and image version
[ ] frontend CPU request and limit
[ ] the scaler entry in apps.json
[ ] profile and scale
[ ] arm
[ ] node the frontend was pinned to
[ ] repetition number in the prefix
[ ] workload.csv
[ ] prometheus.csv
[ ] timeline.json
[ ] scaler.log for the croft arm
[ ] validity status
```

`data/web/runs.jsonl` records the parameters of every run.

---

# 20. Typical session

Log in and enter the harness:

```sh
ssh <user>@<server>
cd ~/daniel/github/cloudx/croft/labs/research/experiment
```

Verify the application:

```sh
just samples smoke otel-demo
```

Calibrate it:

```sh
just samples calibrate otel-demo "10 20 35"
```

Run each arm once:

```sh
just samples burst otel-demo none  otel-try
just samples burst otel-demo keda  otel-try
just samples burst otel-demo croft otel-try
```

Run the series:

```sh
tmux new -s series
just samples series otel-demo otel
```

---

# 21. Clean up

The applications can stay deployed. An idle one uses little CPU.

To remove one:

```sh
just samples remove hotel-reservation
```

After a series, pull croft's datasets for the same window:

```sh
(cd ../../k3s-cluster && ansible-playbook collect.yml)
```

---

# 22. Final output

After a campaign:

```text
data/
├── calibration/
│   ├── hotel-reservation.json
│   └── otel-demo.json
└── web/
    ├── runs.jsonl
    ├── compare.csv
    ├── hr-none-1-burst/
    ├── hr-keda-1-burst/
    ├── hr-croft-1-burst/
    ├── ...
    └── otel-croft-3-burst/
```

Each run directory holds the client, Prometheus and autoscaler observations for one exact window.

`compare.csv` holds one scored row per run, and is the input of the analysis.
