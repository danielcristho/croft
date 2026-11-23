# croft Experiments — Comparing the HPA and croft

This guide is the step-by-step procedure for the **burst autoscaling experiments**.

It compares two autoscalers on the same applications, under the same load, on the same node:

- the Kubernetes **HPA**, scaling on CPU from metrics-server;
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
         +---- HPA / croft  ---->  timeline.json, scaler.log
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

# 2. The two arms

Each arm is one value of `--scaler`.

| Arm | Who adds replicas | Signal | Signal path |
|---|---|---|---|
| `none` | the HPA from `hpa.yaml` | CPU at 70% of the request | metrics-server, checked every 15 s |
| `croft` | `croft scale`, with no HPA | the calibrated syscall rate | collector `/metrics` every 2 s |

Both arms scale down after a 300 s window and share one capacity model:

```text
a pod is full at 70% of its CPU limit
```

The arms therefore differ only in **their signal** and **how late it arrives**.

---

# 3. Prerequisites

The cluster must provide:

```text
k3s cluster from labs/k3s-cluster
ingress-nginx on NodePort 30080
kube-prometheus-stack
croft collector DaemonSet with CROFT_METRICS_ADDR set   (croft arm)
```

The machine running the harness needs:

```text
uv
kubectl
just
```

Run the harness on the k3s server node.

`apps.json` reaches the applications at `127.0.0.1`, and the load generator should not share a network path with anything else.

---

# 4. Enter the harness

Log in to the server:

```sh
ssh <user>@<server>
cd croft/labs/research/experiment
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

Choose the node that runs the pods of every experiment:

```sh
kubectl get nodes
export CROFT_NODE=<node>
```

The recipes also read it from `.local/env` at the repository root, a gitignored file for settings of one checkout.

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

Another machine can follow a run with a kubeconfig that points at the server's reachable address, as in [the cluster guide](../../k3s-cluster/README.md).

---

# 6. Prepare the applications

Three applications are available:

| Application | Shape | CPU per request | Pod CPU limit | `--scale` |
|---|---|---:|---:|---:|
| `hotel-reservation` | DeathStarBench, 8 Go services, MongoDB, memcached | 6.2 ms at the frontend | 200m | 1 |
| `online-boutique` | Online Boutique, a Go frontend over 10 services, Redis | 3.3 ms at the frontend | 200m | 1 |
| `otel-demo` | OpenTelemetry Demo, Node.js, Python, Go and C++ services, PostgreSQL | 18.3 ms at the frontend | 300m | 0.55 |

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

# 7. Enable the ingress metrics

The dashboard and the offered-load check read the request rate that the ingress reports through Prometheus.

Set it up once per cluster.

Let ingress-nginx export per-ingress metrics on port 10254:

```sh
kubectl -n ingress-nginx patch deployment ingress-nginx-controller --type=json -p '[
  {"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--enable-metrics=true"},
  {"op":"add","path":"/spec/template/spec/containers/0/ports/-","value":{"name":"metrics","containerPort":10254,"protocol":"TCP"}}]'
```

The controller restarts for a few seconds.

Scrape the metrics every 5 s instead of the stack's 30 s:

```sh
kubectl apply -f workloads/ingress-nginx/podmonitor.yaml
```

Check that Prometheus has the request counts:

```sh
curl -s http://prometheus.127.0.0.1.sslip.io:30080/api/v1/query \
  --data-urlencode 'query=sum by (service) (rate(nginx_ingress_controller_requests[30s]))'
```

The answer must list a `service` for each application that received traffic.

---

# 8. Calibrate an application

The croft arm needs each application's `scaler` entry in `apps.json`:

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
deploys the application at one replica, pinned to `CROFT_NODE`, without an autoscaler
offers each level as constant load
reads the pod's syscall rates and CPU from the collector on its node
reads the busiest pods of the namespace from metrics-server
```

A successful calibration resembles:

```text
otel-demo: frontend-66f497f847-4mbx2 on node-1, CPU limit 1
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
| `hotel-reservation` | `write` | 11.8 | 6.18 |
| `online-boutique` | `getpeername` | 2.0 | 3.26 |
| `otel-demo` | `writev` | 4.83 | 18.26 |

The Go and Node.js frontends answer through `write` and `writev`. Take the syscall the calibration picks, not the one another application uses.

Online Boutique's frontend calls `getpeername` twice per request at 5, 10 and 20 RPS, within 0.1%; `write` runs at about 38 per request and spreads 3.5%. Check in its first croft burst that croft's estimated RPS follows the client's.

The `otel-demo` values were measured at its 300m limit, at 5, 10 and 15 RPS. `writev` per request moved 4.5% across the levels. CPU per request fell from 21.4 to 15.0 ms, because the Node.js frontend spends about 50m whatever the load; each further request costs about 12 ms. The mean, 18.26 ms, therefore puts one pod's capacity at about 12 RPS at 70%, a little below the 13.5 RPS a line through the levels gives, so croft scales slightly early. 20 RPS ran the frontend at 91% of its limit and stops the calibration.

An earlier calibration measured 7.1 ms at 10 RPS; this one 18.4 ms at 10 RPS, and a run at 10 and 20 RPS 17.6 ms. hotel-reservation rose from 1.8 to 6.2 ms in the same period. The collector changed between them, to `croft-collector:operands`. That its probes cost the frontend this CPU is not verified.

---

# 9. Validate a calibration

Accept a calibration only when, at every level:

```text
[ ] achieved RPS equals the target
[ ] failed is 0
[ ] p95 stays flat
[ ] the frontend's CPU stays below 85% of its limit; throttling alone does not count
[ ] no other pod of the namespace nears its own CPU limit
```

Otherwise the levels are too high, or a backend service saturates before the frontend.

`calibrate-scaler` stops at a level with failed requests, one that answers less than 95% of what was sent, or one where the frontend reaches 85% of its CPU limit. A pod at its limit still answers every request, but later, so its CPU per request comes out too low. `otel-demo`'s frontend answered all of 20 RPS at 91% of its 300m limit, at 13.6 ms per request against 17.6 ms at 10 RPS.

Extrapolate the backends to the burst's peak too. In Online Boutique, `currencyservice` used 0.067 core at 20 RPS, a third of its 200m limit and more than the frontend; at 150 RPS it would need about 0.5 core at one replica and cap the burst. Its limit is 1 core for that reason.

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
| `burst` | 10 RPS, 150 RPS from 180 s to 300 s, then 10 RPS | 900 s | reaction, recovery and scale-down |
| `periodic` | 10 to 100 RPS, period 120 s | 600 s | autoscaling lag |
| `variable` | 10 to 100 RPS, a new level every 30 s, seed 42 | 600 s | prediction |

`--scale` multiplies every rate. It defaults to the application's `scale` in `apps.json`.

A good burst saturates one pod at least twice over and stays within ten pods:

```text
hotel-reservation  --scale 1    10 → 150 → 10 RPS    23 RPS per pod    about 7 pods
otel-demo          --scale 0.55 5.5 → 82.5 → 5.5 RPS 12 RPS per pod    about 7 pods
online-boutique    --scale 1    10 → 150 → 10 RPS    43 RPS per pod    about 4 pods
```

For a new application or a changed manifest, find one pod's capacity first with a ramp at a fixed replica count:

```sh
uv run run.py web --app hotel-reservation --prefix hr-ramp --profile ramp --scale 1 --replicas 1
```

Read the RPS at which p95 leaves its floor.

`--replicas N` runs no autoscaler: the frontend stays at N pods and every other Deployment at its manifest's count. Use it to see which services take a burst.

## Endpoint mix

`--mix` sends the application's weighted endpoints from `apps.json` instead of its one path, so services off that path take load too: Boutique's checkout, payment and shipping, for example. The final series keeps the one path.

A mix costs a different CPU per request than the path, so it needs its own scale and calibration before a burst:

1. Check that every endpoint answers: `uv run run.py web --app <app> --prefix <app>-mix-smoke --profile constant --mix --replicas 1`. `endpoints.csv` should show no failures.
2. Find one pod's capacity: `uv run run.py web --app <app> --prefix <app>-mix-ramp --profile ramp --mix --replicas 1`.
3. Calibrate: `uv run run.py calibrate-scaler --app <app> --mix --node <node>`, and compare `data/calibration/<app>-mix.json` with `<app>.json`.
4. Set `mix.scale` so the burst needs about six to eight pods, and `mix.scaler` when the calibration moved.

hotel-reservation answers a failed login with 200, so `endpoints.csv` cannot show it; check a reserve once by hand after changing its user.

---

# 12. Run a first burst

Run each arm once on the application before a series:

```sh
just samples burst otel-demo none   otel-try
just samples burst otel-demo croft  otel-try
```

Each arm lands in its own directory, `data/web/otel-try-<arm>-burst/`.

The recipe expands to:

```sh
uv run run.py web --app otel-demo --node node-1 --prefix otel-try-none \
  --profile burst --probes relaxed --scaler none
```

`--node node-1` pins every replica to one node.

The nodes differ by up to 1.69 times in CPU speed, and the scheduler otherwise places the first pod anywhere. In the smoke runs a single frontend on the fastest node held the 300 RPS hotel burst at a 143 ms median p95, while a pod on the slowest reached 1,181 ms.

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
├── endpoints.csv      the same per endpoint; one endpoint without --mix
├── prometheus.csv     CPU, throttled share, memory, network, restarts and replicas, every 5 s
├── services.csv       the same for every Deployment, with its role and language from apps.json
├── signals.csv        collector series of every service: syscalls, bytes, socket I/O, connects, CPU
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
```

Sign in as `admin` with the password from:

```sh
kubectl -n monitoring get secret monitoring-grafana -o jsonpath='{.data.admin-password}' | base64 -d; echo
```

Choose the application, and the syscall from its calibration: `write` for hotel-reservation, `writev` for otel-demo, `getpeername` for online-boutique.

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
otel-none-1   otel-croft-1
otel-none-2   otel-croft-2
otel-none-3   otel-croft-3
```

and scores them at the end.

A series takes about 1 h 40 min per application.

It outlives an SSH session only under `tmux` or `nohup`:

```sh
tmux new -s series
just samples series hotel-reservation hr
just samples series online-boutique ob
just samples series otel-demo otel
```

The full study is 3 applications × 2 arms × 3 repetitions: 18 runs, about 5 hours of cluster time.

Compare the arms within one application. Absolute latencies differ between the applications by design.

A saturated pod's syscalls show what the pod manages to answer, not the offered load.

---

# 17. Score the runs

Score finished runs by prefix:

```sh
just samples compare otel-none- otel-croft-
```

`compare` groups the runs by app, profile, scale, probes and arm, prints mean ± sd for each group, and writes one row per run to `data/web/compare.csv`.

Rows of runs it scores again are replaced; rows of other runs stay, so scoring one series keeps the others.

| Column | Meaning |
|---|---|
| `hpa_first_up_s` | burst start to the first HPA scale-up |
| `croft_first_up_s` | burst start to croft's first scale-up |
| `first_ready_s` | burst start to the first new pod Ready |
| `peak_replicas`, `time_to_peak_s` | the most replicas asked in the burst, and when it was first asked |
| `scale_down_events` | scale-downs inside the burst |
| `pods_created` | pods added over the run: the sum of the replica increases |
| `slo_violation_s` | seconds with p95 above 500 ms |
| `failed_fraction` | failed over answered requests in the burst |
| `success_ratio` | successes over offered requests in the burst |
| `replica_seconds` | replicas times seconds over the run, the cost |

`[k]` after a value means only k runs of the group have it.

`first_ready_s` reads the pods in `timeline.json`. A run lists the pods still running at its end and adds, from kube-state-metrics in Prometheus, the pods a scale-down deleted before then, with their node and container ids. The run's `containers` in `runs.jsonl` holds those ids too, so collector rows of a deleted pod still join on their container id. Runs recorded before this left the deleted pods out, so their `first_ready_s` is empty. Fill them in while Prometheus still keeps them (10 days), then score again:

```sh
uv run run.py timeline --prefix f2-hr-
uv run run.py compare --prefix f2-hr-none- f2-hr-croft-
```

`peak_replicas`, `time_to_peak_s` and `pods_created` follow every scaling decision: the HPA's events and croft's log. kube-state-metrics is scraped every 30 s and misses a count the HPA undoes seconds after croft asked for it. Kubernetes merges an event's repeats into one with a count, so the repeats are spread evenly over its span, one per 15 s HPA sync. `replica_seconds` still comes from kube-state-metrics.

---

# 18. Study service interactions

This study offers `burst` and `ramp` loads at two scales to the three applications under the HPA, with Linkerd recording every call between their services. It asks which call slows down first, and how the slowdown travels.

It is a separate study from the comparison series: `--mesh` adds a proxy to every pod and works with the HPA arm only.

## Prepare

`just samples mesh-prepare` runs these steps and the capacity ramps below for all three applications, in about an hour, and logs to `data/web/mesh-prepare.log`.

1. Let Prometheus scrape the Linkerd proxies, once per cluster:
   ```sh
   kubectl apply -f workloads/linkerd/podmonitor.yaml
   ```
2. Smoke-test each application with the mesh:
   ```sh
   uv run run.py web --app <app> --node <node> --prefix <app>-mesh-smoke --profile constant --probes relaxed --mesh
   linkerd viz edges deploy -n <namespace>
   ```
   The first `--mesh` run restarts the whole application to add the proxies.
3. Check the smoke run: `edges.csv` has an `ingress → frontend` edge and the calls `linkerd viz edges` lists, `services.csv` has every Deployment, `signals.csv` has the frontend's `syscall_rate`, and `kubectl get hpa -n <namespace>` reads a value.

> **Note**
> The proxy costs CPU and latency. Rerun one burst of each application with `--mesh` next to the [reference run](#15-reference-run), and report both.

## Choose the scales

Find one frontend pod's capacity with the mesh, with a ramp at one replica:

```sh
uv run run.py web --app <app> --node <node> --prefix <app>-mesh-cap --profile ramp --replicas 1 --probes relaxed --mesh
```

Read the RPS at which p95 leaves its floor; `just samples mesh-capacity` prints the p95 per rate and suggests a knee. Choose `lo` so the burst peak is about twice that capacity, and `hi` about four times, within ten pods. Record both, and the reason, before the series: they are chosen from the data.

| Application | One pod holds | `lo` | `hi` |
|---|---:|---:|---:|
| `hotel-reservation` | | | |
| `online-boutique` | | | |
| `otel-demo` | | | |

## Run the series

3 applications × 2 profiles × 2 scales × 3 repetitions: 36 runs, about 10 hours. Every repetition goes through every group once, so drift falls on all of them:

```sh
declare -A SCALE=([hotel-reservation-lo]=… [hotel-reservation-hi]=… [online-boutique-lo]=… [online-boutique-hi]=… [otel-demo-lo]=… [otel-demo-hi]=…)
for i in 1 2 3; do for app in hotel-reservation online-boutique otel-demo; do for p in burst ramp; do for s in lo hi; do
  uv run run.py web --app $app --node <node> --prefix si-$app-$s-$i --profile $p --scale ${SCALE[$app-$s]} --probes relaxed --mesh || break 4
done; done; done; done
```

`just samples interactions <series> <hr-lo> <hr-hi> <ob-lo> <ob-hi> <otel-lo> <otel-hi>` runs this loop, with `<series>` in place of `si` in the run names. A name already recorded in `runs.jsonl` resumes that series, so a repeat of the study needs a new name such as `si2`. It logs to `data/web/interactions.log`, records the scales in `data/web/interactions-scales.txt`, and when rerun after a failure it skips the runs already recorded. `just samples interactions-check` prints mesh, edges, backend restarts and ingress p95 per run.

Run it under `tmux` and read its output: a pipe through `grep` or `tail` hides a failed run. Afterwards, check that `data/web/<series>-*` holds 36 directories. Read `failed_pct` in `just samples interactions-check <series>` too: ingress p95 can look healthy while requests fail fast.

A comparison run after the series, without `--mesh`, removes the proxies and restarts the application first.

## Run the three applications at once

`--concurrent` lets runs of different applications share the cluster, which takes the study from about 10 hours to about 4. Pin each frontend to its own node, and run one `tmux` window per application:

```sh
just samples mesh-prepare-app hotel-reservation <node-a>
just samples mesh-prepare-app online-boutique <node-b>
just samples mesh-prepare-app otel-demo <node-c>
```

Once all three ramps finish, read `just samples mesh-capacity`. Each application's `lo` and `hi` come from its own ramp, so from its own node, since the nodes differ by up to 1.69 times. Then, one window each:

```sh
just samples interactions-app <series> hotel-reservation <node-a> <lo> <hi>
just samples interactions-app <series> online-boutique <node-b> <lo> <hi>
just samples interactions-app <series> otel-demo <node-c> <lo> <hi>
```

Each window runs that application's 12 runs (about 3 hours), logs to `data/web/interactions-<app>.log`, and resumes the same way after a failure. A run without `--concurrent` waits until all three have finished, and the other way round.

> **Note**
> Concurrent runs still share ingress-nginx, Prometheus, the load generator and the nodes the backends land on, and the windows drift out of step, so one application's burst can fall on another's run. Compare concurrent runs only with concurrent runs, and report that they shared the cluster; `interactions-check` and `runs.jsonl` mark them.

## Accept a run

Accept a run as in [Accept or reject a run](#14-accept-or-reject-a-run), and also:

- No backend restarted (`restarts` per service in `services.csv`).
- No edge of `linkerd viz edges` is missing from `edges.csv` for the whole window.
- Failures inside the mesh show as a `success_rate` below 1; they are data, not a reason to reject the run.

## Load map

The load map (#21) asks which backend takes a burst once the frontend does not hold it back. The frontend is fixed at the peak count the HPA reached in the `si` bursts, and every backend at its manifest's one pod. The load is the application's one path, as in the comparison series:

```sh
for i in 1 2 3; do
  uv run run.py web --app <app> --node <node> --prefix lm-<app>-$i --profile burst --scale <lo> --replicas <N> --probes relaxed --mesh || break
done
just samples loadmap lm-<app>
```

| Application | `--replicas` | `--scale` |
|---|---:|---:|
| `hotel-reservation` | 7 | 1.33 |
| `online-boutique` | 6 | 0.80 |
| `otel-demo` | after #28 | after #28 |

`just samples loadmap <prefix>` prints, per application and scale, every service's peak throttled share, peak and base CPU, syscall rise (burst over base, from `signals.csv`) and restarts, then the busiest edges with their p95 before and during the burst. Base is the minute before the burst. Support components are left out. The order, throttling first and then CPU, was chosen before the load-map runs; it is a heuristic. A backend restart is a result here, not a reason to reject the run.

---

# 19. Troubleshooting

| Message | Cause and fix |
|---|---|
| `<image> is missing on <nodes>` | Load the image on those nodes, see [workloads/README.md](workloads/README.md). The node status can take a few minutes to list it. |
| `another experiment holds data/experiment.lock` | A run is in progress. Wait for it, or stop it. |
| `apps.json has no calibration for <app>` | Measure one, see [Calibrate an application](#8-calibrate-an-application). |
| `kubectl get nodes … returned non-zero exit status` | `KUBECONFIG` is not set in this shell. |
| `<host>/ did not answer 200 within 120 s` | The ingress has no Ready backend. Check `just samples status <app>`. |

---

# 20. Reproducibility checklist

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

# 21. Typical session

Log in and enter the harness:

```sh
ssh <user>@<server>
cd croft/labs/research/experiment
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
just samples burst otel-demo croft otel-try
```

Run the series:

```sh
tmux new -s series
just samples series otel-demo otel
```

---

# 22. Clean up

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

# 23. Final output

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
    ├── hr-croft-1-burst/
    ├── ...
    └── otel-croft-3-burst/
```

Each run directory holds the client, Prometheus and autoscaler observations for one exact window.

`compare.csv` holds one scored row per run, and is the input of the analysis.
