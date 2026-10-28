# croft experiments — Running the Web Applications

This guide is the step-by-step procedure for running **burst experiments** against the two web applications of `apps.json`:

| Application | Architecture / dominant behavior | Typical observations |
|---|---|---|
| `webapp` | One CPU-bound HTTP service, 40 to 65 ms of CPU per request | CPU saturation, throttling, HPA lag |
| `hotel-reservation` | DeathStarBench hotelReservation: 8 Go microservices over gRPC, MongoDB, memcached | service-chain latency, a light frontend in front of a deeper call graph |

The same harness, the same load profiles and the same scoring apply to both, so the **application** can be treated as an independent variable.

`README.md` explains what the harness records and why; this guide only says how to run it.

---

# 1. Purpose

Every run asks one question of one application:

> When the offered load bursts, how quickly do the autoscaler and croft react, and how much does the application degrade before they do?

```text
open-loop burst (rps.py)
       |
       v
ingress-nginx :30080
       |
       v
application pods ---------------------------+
       |                                    |
       +--> croft: syscalls, PSI,           +--> client: latency, failures
       |    throttling, every 2 s                (workload.csv)
       |
       +--> HPA: CPU utilization, every 15 s --> replicas (timeline.json)
       |
       +--> Prometheus series (prometheus.csv)
```

---

# 2. The two applications

## webapp

```text
workload
   ↓
ingress (web.<ip>.sslip.io)
   ↓
webapp  /cpu
```

One service, built from `workloads/webapp`. A request burns 40 to 65 ms of CPU, so the app saturates at a few tens of RPS per pod. It requests and is limited to one core.

## hotel-reservation

```text
workload
   ↓
ingress (hotel.<ip>.sslip.io)
   ↓
frontend  /hotels?inDate=…&outDate=…&lat=…&lon=…
   ├──> search ──> geo ──> mongodb-geo
   │         └──> rate ──> memcached-rate, mongodb-rate
   ├──> reservation ──> memcached-reserve, mongodb-reservation
   └──> profile ──> memcached-profile, mongodb-profile

consul (service discovery), jaeger (1% of traces)
```

Every request is a hotel search, which touches five Go services. The frontend costs about 1.1 ms of CPU per request and the whole chain about 6 ms, measured at 50 RPS. The frontend requests and is limited to 200m, so one pod handles about 130 RPS at the HPA's 70% target and throttles above it. The HPA scales the frontend only; the other services stay at one replica.

The data lives in `emptyDir`: every service seeds its MongoDB collection at startup, so a restarted MongoDB pod is empty until its service restarts as well (section 18).

---

# 3. Directory structure

```text
labs/research/experiment/
├── RUNNING.md                     this guide
├── README.md                      what is recorded and why
├── apps.json                      namespace, Deployment, host, path, health of each app
├── run.py                         command line
├── workloads/
│   ├── webapp/                    app.py, Dockerfile, k8s.yaml, hpa.yaml
│   └── hotel-reservation/         k8s.yaml (19 Deployments), hpa.yaml
└── data/web/
    ├── runs.jsonl                 one line per run
    ├── compare.csv                one scored row per burst run
    └── <run>/                     workload.csv, prometheus.csv, timeline.json, scaler.log
```

Enter the directory:

```sh
cd labs/research/experiment
```

---

# 4. Prerequisites

The environment is expected to provide:

```text
k3s cluster from labs/k3s-cluster (3 nodes)
ingress-nginx on NodePort 30080
kube-prometheus-stack, reachable at prometheus.<ip>.sslip.io:30080
croft collector DaemonSet with CROFT_METRICS_ADDR
uv
```

Verify:

```sh
uv sync
export KUBECONFIG=~/.kube/croft-k3s.yaml
kubectl get nodes
```

Every node must be `Ready`.

**Run the harness on the server (node-a).** `apps.json` and `prometheus.py` address the cluster as `127.0.0.1`, and the load generator should not share a network path with anything else. From a laptop, log in first:

```sh
ssh <user>@<server>
```

A laptop on the tailnet can still watch: `kubectl` with `~/.kube/croft-k3s-tailscale.yaml`, Grafana and Prometheus at `*.100.64.0.10.sslip.io:30080`.

---

# 5. Load the images on every node

The harness stops before a run when a node lacks the application's image, because a pod scheduled there would hang in `ImagePullBackOff`.

## webapp

Its image is in no registry; build it and load it:

```sh
docker build -t croft-webapp:v1 workloads/webapp
(cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=croft-webapp:v1 --ask-become-pass)
```

## hotel-reservation

Its images are on Docker Hub, but `deathstarbench/hotel-reservation:0.3.5` is 1.2 GB; pull it everywhere once:

```sh
docker pull deathstarbench/hotel-reservation:0.3.5
(cd ../../k3s-cluster && ansible-playbook load-image.yml -e image=deathstarbench/hotel-reservation:0.3.5 --ask-become-pass)
```

MongoDB, memcached, consul and jaeger are small and pulled on first deploy.

The harness checks every node before each run and names the ones still missing the image. The node status can take a few minutes to list a loaded image.

---

# 6. Deploy and smoke-test by hand

The harness redeploys the app at the start of every run. Deploy by hand once, before the first run, to see that the app answers.

## webapp

```sh
kubectl apply -f workloads/webapp/k8s.yaml
kubectl -n croft-exp rollout status deployment/webapp
curl -i "http://web.127.0.0.1.sslip.io:30080/cpu"
```

## hotel-reservation

```sh
kubectl apply -f workloads/hotel-reservation/k8s.yaml
kubectl -n hotel-res wait --for=condition=Available deployment --all --timeout=600s
curl -i "http://hotel.127.0.0.1.sslip.io:30080/hotels?inDate=2015-04-09&outDate=2015-04-10&lat=38.0235&lon=-122.095"
```

A successful search returns a GeoJSON `FeatureCollection` of hotels.

The Go services restart once or twice on first deploy, while MongoDB and consul start. That is expected; a service still restarting after two minutes is not.

Do not begin a run until the smoke test succeeds.

---

# 7. Understand the run phases

Every `--profile` run consists of:

```text
time ----------------------------------------------------------------------->

 deploy        warm-up          measurement (profile)        export    cool-down
|--------|----------------|------------------------------|---------|-----------|
 1 fresh   30 s at 5 RPS     open-loop RPS, 600 s for       Prom,     15 s
 replica,                     burst/ramp                     events,
 new HPA                                                     pods
```

## Deploy

The app's Deployment is applied, pinned or unpinned, patched with the chosen probes, scaled to one replica and restarted, then its HPA is applied. Every run therefore starts from the same state: one pod, no scale-down window, no queued requests.

## Warm-up

Connections, caches and the HPA's first metrics settle. Warm-up traffic is not in `workload.csv`.

## Measurement

The profile runs open loop: requests leave at the target rate whether or not earlier ones were answered.

## Export

`prometheus.csv` (CPU, throttling, memory, network, restarts, replicas), `timeline.json` (HPA rescales, replica set changes, pod created and Ready times) and the run line in `runs.jsonl`.

## Cool-down

Separates consecutive runs.

---

# 8. Load profiles and `--scale`

| Profile | Shape (at `--scale 1`) | Duration | Typical research purpose |
|---|---|---|---|
| `constant` | 25 RPS | 30 s | validation |
| `ramp` | 10 → 200 RPS | 600 s | find the saturation region |
| `burst` | 10 RPS, 150 RPS from 180 s to 300 s, 10 RPS | 600 s | reaction and recovery |
| `periodic` | 10 to 100 RPS, period 120 s | 600 s | autoscaling lag |
| `variable` | 10 to 100 RPS, new level every 30 s, seed 42 | 600 s | prediction |

`--scale` multiplies every rate. The two apps differ by about fifty times in CPU per request, so each has its own scale:

| Application | `--scale` | Burst | Why |
|---|---:|---|---|
| `webapp` | 0.5 | 5 → 75 → 5 RPS | 75 RPS needs 6 to 7 pods at 70%, within the HPA's 10 |
| `hotel-reservation` | 2 | 20 → 300 → 20 RPS | 300 RPS is ~2.3 frontend pods' worth; one pod throttles until the HPA adds more |

Keep the scale fixed within a comparison. A scale that never saturates one pod shows nothing; one that needs more than ten pods shows only the HPA's maximum.

Pattern of a burst:

```text
RPS
 ^
 |               +-----------+
 |               |           |
 |_______________|           |_______________
 +----------------------------------------------> time
 0              180         300             600 s
```

---

# 9. Run the first validation burst

```sh
uv run run.py web --app webapp --prefix try --profile burst --scale 0.5 --probes relaxed
uv run run.py web --app hotel-reservation --prefix hr-try --profile burst --scale 2 --probes relaxed
```

Each takes about 12 minutes. Only one run holds the cluster at a time (`data/experiment.lock`); a second command started meanwhile exits at once with `another experiment holds …`.

`--probes relaxed` keeps an overloaded pod alive and in its Service: no liveness restart, and readiness tolerates 60 s of slow answers. Use it for every comparison, so a restart does not hide the burst.

The result appears under:

```text
data/web/try-burst/
data/web/hr-try-burst/
```

A reference `hotel-reservation` run (`--scale 2`, `--probes relaxed`, HPA alone) gave:

| | Before the burst | Burst | After |
|---|---:|---:|---:|
| target / achieved RPS | 20 / 20 | 300 / 297 | 20 / 21 |
| median p95 latency | 12 ms | 1,181 ms | 13 ms |
| seconds with p95 > 500 ms | 1 | 72 | 0 |
| frontend CPU throttled | 0% | 44 to 92% | 0% |

The HPA scaled up 35 s into the burst and reached 3 replicas at 114 s; no request failed and no pod restarted. A run far from this shape points at the setup, not the autoscaler.

---

# 10. Inspect the result

```sh
ls -lah data/web/hr-try-burst/
head data/web/hr-try-burst/workload.csv
tail -1 data/web/runs.jsonl | python3 -m json.tool
```

The directory contains:

```text
workload.csv      target and achieved RPS, successes, failures, mean and p95 latency, per second
prometheus.csv    cpu_cores, cpu_throttled_fraction, memory_bytes, network, restarts, replicas, every 5 s
timeline.json     HPA rescales, replica set changes, pod created and Ready times
scaler.log        only with --scaler croft: croft's estimate every poll and its decisions
```

`workload.csv` columns:

```text
timestamp_epoch
elapsed_s
target_rps
sent
success
failed
achieved_rps
mean_latency_ms
p95_latency_ms
```

---

# 11. Validate the run

Do not accept a run because the command finished. Verify:

```text
[ ] achieved_rps follows target_rps outside the burst
[ ] the burst saturated one pod: p95 rose, or throttling appeared, before the HPA reacted
[ ] the HPA scaled up (timeline.json "rescales") and stayed within 10 replicas
[ ] failures are understood (timeouts during saturation are data; 404s are not)
[ ] no pod restarted unexpectedly (prometheus.csv restarts)
[ ] Prometheus answered for the whole window (no empty columns)
[ ] no other experiment or workload ran on the cluster at the same time
```

At low load:

```text
target_rps ≈ achieved_rps,  failed = 0
```

is expected. For `hotel-reservation`, also check that every service other than the frontend stayed well under its 1-core limit (`kubectl top pods -n hotel-res` during a run): the experiment is about the frontend's scaling, and a saturated `rate` or `search` would move the bottleneck.

---

# 12. Repeat

A single run is not representative. Three repetitions per arm is the minimum; five allow a confidence interval.

```sh
for i in 1 2 3; do
  uv run run.py web --app hotel-reservation --prefix hr-hpa-$i --profile burst --scale 2 --probes relaxed || break
done
```

Name runs `<app>-<arm>-<repetition>`: the run id is `<prefix>-<profile>`, and a rerun under the same id overwrites its directory.

---

# 13. Find the operating range with a ramp

Before trusting a scale for a new application or a changed manifest, run a ramp at fixed replicas:

```sh
uv run run.py web --app hotel-reservation --prefix hr-ramp --profile ramp --scale 2 --replicas 1
```

```text
RPS
 ^                                /
 |                             /
 |                          /       <- p95 breaks 500 ms here: one pod's capacity
 |                       /
 |____________________/__________________> time
```

The RPS at which p95 leaves its floor is one pod's capacity. The burst rate should exceed it by two to three times, and stay under ten pods' worth.

---

# 14. HPA alone against HPA + croft

The comparison pairs two arms under identical conditions:

```text
                 identical burst
                        |
           +------------+------------+
           |                         |
           v                         v
      HPA alone              HPA + croft scale
  (--scaler none)            (--scaler croft)
           |                         |
           +------------+------------+
                        |
                        v
               run.py compare
```

For webapp:

```sh
for i in 1 2 3; do
  uv run run.py web --app webapp --prefix web-hpa-$i --profile burst --scale 0.5 --probes relaxed || break
  uv run run.py web --app webapp --prefix web-duo-$i --profile burst --scale 0.5 --probes relaxed --scaler croft || break
done
uv run run.py compare --prefix web-hpa- web-duo-
```

Alternate the arms inside the loop, as above, so drift over the afternoon falls on both.

`--scaler croft` needs the app's calibration in `apps.json` (`scaler`: syscall, calls and CPU milliseconds per request):

| Application | Syscall | Calls per request | CPU per request |
|---|---|---:|---:|
| `webapp` | `sendto` | 2.06 | 50 ms |
| `hotel-reservation` | `write` | 11.4 | 1.6 ms |

The hotel frontend is Go, which answers through `write`, not `sendto`. Its calibration comes from one frontend pod, without the HPA, at 20, 50 and 100 RPS for 75 s each, read from the collector's `/metrics`: `write` per request moved 1% across the levels, CPU per request from 1.8 to 1.55 ms. Recalibrate after changing the image or the frontend's resources.

---

# 15. Score the runs

```sh
uv run run.py compare --prefix web-hpa- web-duo- hr-hpa-
```

`compare` groups runs by arm (app, profile, scale, probes, scaler), prints mean ± sd per arm and writes one row per run to `data/web/compare.csv`:

| Column | Meaning |
|---|---|
| `hpa_first_up_s` | burst start to the HPA's first scale-up |
| `croft_first_up_s` | burst start to croft's first scale-up |
| `first_ready_s` | burst start to the first new pod Ready |
| `peak_replicas`, `time_to_peak_s` | the most replicas, and when |
| `scale_down_events` | replica scale-downs inside the burst (the HPA undoing croft) |
| `slo_violation_s` | seconds with p95 above 500 ms |
| `failed_fraction` | failed over answered requests in the burst |
| `success_ratio` | successes over offered requests in the burst |
| `replica_seconds` | replicas × seconds over the run: the cost |

`[k]` after a value means only k runs of the arm have it.

---

# 16. Comparative study across the two applications

> Does an early, kernel-level signal help more for a heavy single service or for a light frontend in front of a service chain?

| ID | Application | Arm | Profile | Scale | Repetitions |
|---|---|---|---|---:|---:|
| E1 | webapp | HPA | burst | 0.5 | 3 |
| E2 | webapp | HPA + croft | burst | 0.5 | 3 |
| E3 | hotel-reservation | HPA | burst | 2 | 3 |
| E4 | hotel-reservation | HPA + croft | burst | 2 | 3 |

Controlled across all runs:

```text
profile and its duration
--probes relaxed
HPA: CPU 70%, 1 to 10 replicas
cluster topology, croft collector version
no other workload on the cluster
```

Do not compare absolute latencies between the applications: they differ by design. Compare reaction times, SLO seconds and replica-seconds within each application, and the size of croft's effect across them.

---

# 17. Accepting and rejecting runs

Reject, or label separately, a run when:

```text
a pod restarted for a reason other than the burst
the load generator could not keep target_rps outside the burst
Prometheus or the croft collector stopped
the manifest or apps.json changed between repetitions
another experiment, a build or a GPU job ran at the same time
```

Do not silently include such runs in `compare`; pass prefixes that leave them out.

---

# 18. Switching and removing applications

Both applications can stay deployed: an idle app uses little CPU. During a run, only the app under test receives load.

Reset `hotel-reservation`'s data, for example after a MongoDB pod restarted:

```sh
kubectl -n hotel-res rollout restart deployment
kubectl -n hotel-res wait --for=condition=Available deployment --all --timeout=600s
```

Remove an application:

```sh
kubectl delete namespace hotel-res
kubectl delete -f workloads/webapp/k8s.yaml
```

---

# 19. Typical session on the server

```sh
ssh <user>@<server>
cd ~/daniel/github/cloudx/croft/labs/research/experiment
export KUBECONFIG=~/.kube/croft-k3s.yaml

kubectl get nodes
curl -s -o /dev/null -w "%{http_code}\n" "http://hotel.127.0.0.1.sslip.io:30080/"

for i in 1 2 3; do
  uv run run.py web --app hotel-reservation --prefix hr-hpa-$i --profile burst --scale 2 --probes relaxed || break
done
uv run run.py compare --prefix hr-hpa-
```

Long series outlive an SSH session; run them under `tmux` or `nohup`.

Afterwards, pull croft's datasets for the same window:

```sh
(cd ../../k3s-cluster && ansible-playbook collect.yml)
```

---

# 20. Reproducibility

For every accepted run preserve:

```text
[ ] run id and repetition
[ ] application and image version (apps.json, workloads/<app>/k8s.yaml at that commit)
[ ] resource requests and limits, HPA target
[ ] profile, --scale, --probes, --scaler
[ ] workload.csv, prometheus.csv, timeline.json, scaler.log
[ ] the run line in runs.jsonl
[ ] croft's dataset for the window
[ ] validity status
```

Do not edit a finished run directory. Change the configuration and run again under a new prefix.
