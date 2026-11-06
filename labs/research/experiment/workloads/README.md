# croft Experiments — Applications Under Test

This folder provides the applications that the burst experiments run against.

The applications differ in language, depth of the service chain, and CPU cost per request. They allow the same burst procedure and the same three autoscalers to be compared across application architectures.

Each application is a folder here with:

```text
k8s.yaml    the application, in its own namespace, with an ingress
hpa.yaml    its baseline autoscaler: the frontend at 70% CPU, 1 to 10 replicas
```

and an entry in [`apps.json`](../apps.json) that tells the harness where it runs and which request to send.

Every recipe below runs from the repository root as `just samples <recipe>`, or from `labs/research/experiment` as `just <recipe>`. The experiments themselves are in [RUNNING.md](../RUNNING.md).

## Included applications

| Application | Source | Request path | Services | `--scale` |
|---|---|---|---:|---:|
| `hotel-reservation` | [DeathStarBench](https://github.com/delimitrou/DeathStarBench) hotelReservation, image 0.3.5 | `/hotels`: frontend, search, geo, rate, reservation, profile (Go, gRPC) | 19 | 2 |
| `online-boutique` | [Online Boutique](https://github.com/GoogleCloudPlatform/microservices-demo) v0.10.7 | `/`: frontend (Go), productcatalog, currency, cart, recommendation, ad (gRPC) | 12 | 1 |
| `otel-demo` | [OpenTelemetry Demo](https://github.com/open-telemetry/opentelemetry-demo) 3.1.0 | `/api/recommendations`: frontend (Node.js), recommendation (Python), product-catalog (Go, PostgreSQL), currency (C++) | 15 | 1 |

The HPA and croft scale each application's frontend. The services behind it stay at one replica.

## Directory structure

```text
workloads/
├── README.md
├── hotel-reservation/
│   ├── k8s.yaml
│   └── hpa.yaml
├── online-boutique/
│   ├── k8s.yaml
│   └── hpa.yaml
├── otel-demo/
│   ├── k8s.yaml
│   └── hpa.yaml
├── ingress-nginx/
│   └── podmonitor.yaml     request counts
├── monitoring/
│   ├── kustomization.yaml
│   ├── dashboard.json      the Grafana dashboard of a run
│   └── collector-podmonitor.yaml
├── linkerd/
├── gpu/
└── frameworks/
```

---

# 1. Prerequisites

The environment is expected to provide:

```text
the lab cluster from labs/k3s-cluster
ingress-nginx on NodePort 30080
just
kubectl
curl
Python 3
```

Verify:

```sh
just --version
kubectl get nodes
```

---

# 2. Environment configuration

The recipes point `kubectl` at:

```text
~/.kube/croft-k3s.yaml
```

To use another cluster, export it once:

```sh
export KUBECONFIG=~/.kube/croft-k3s-tailscale.yaml
```

The applications answer on two hosts:

```text
<name>.127.0.0.1.sslip.io:30080        on the server
<name>.100.64.0.10.sslip.io:30080     over Tailscale
```

List the applications, their namespaces and request URLs:

```sh
just samples apps
```

---

# 3. Deploy an application

The harness redeploys an application at the start of every run.

Deploy it by hand once, before the first run, to pull its images and see that it answers:

```sh
just samples deploy hotel-reservation
```

The recipe applies `k8s.yaml` and waits until every Deployment is Available.

---

# 4. Verify the deployment

Inspect the pods and their nodes:

```sh
just samples status hotel-reservation
```

Every pod must be `Running` and `Ready`.

---

# 5. Smoke-test the application

Send one request on the experiment path:

```sh
just samples smoke hotel-reservation
```

A successful smoke test resembles:

```text
200 0.014s
```

Print the request URL, on the server and over Tailscale:

```sh
just samples url hotel-reservation
```

Do not start an experiment until the smoke test returns `200`.

---

# 6. online-boutique

[Online Boutique](https://github.com/GoogleCloudPlatform/microservices-demo) v0.10.7, Google Cloud's microservice demo.

Every request is the home page:

```text
frontend (Go) → productcatalog
              → currency
              → cart → Redis
              → recommendation
              → ad
```

`k8s.yaml` is the release's `kubernetes-manifests.yaml` with these changes:

```text
no load generator and no external LoadBalancer
an ingress on boutique.127.0.0.1.sslip.io and boutique.100.64.0.10.sslip.io
the frontend's CPU request equal to its 200m limit, and 256Mi of memory
replicas: 1 on every other Deployment
    the namespace was once scaled to zero; a deploy restores the backends
a 1-core CPU limit on currencyservice, not 200m
    it stays at one replica and would otherwise saturate before the frontend
```

The frontend costs about 3.3 ms of CPU per request: one pod holds about 43 RPS at 70%, so a burst at `--scale 1` needs about four pods.

Earlier versions of this lab ran Online Boutique as background load, with an HPA on every service. Remove those HPAs once, so only the frontend scales:

```sh
kubectl -n boutique delete hpa --all
```

Deploy and verify:

```sh
just samples deploy online-boutique
just samples smoke online-boutique
```

The harness checks that the frontend image is on every node. Load it if a run stops on it:

```sh
docker pull us-central1-docker.pkg.dev/online-boutique-ci/microservices-demo/frontend:v0.10.7
just samples load-image us-central1-docker.pkg.dev/online-boutique-ci/microservices-demo/frontend:v0.10.7
```

---

# 7. hotel-reservation

[DeathStarBench](https://github.com/delimitrou/DeathStarBench) hotelReservation.

Every request is a hotel search:

```text
frontend → search → geo
                  → rate
         → reservation
         → profile
```

The frontend is limited to 200m and costs 1.6 ms of CPU per request.

The harness checks that the first image of `k8s.yaml` is on every node. The 1.2 GB `deathstarbench/hotel-reservation:0.3.5` is otherwise only pulled where a pod lands.

Load it everywhere once:

```sh
docker pull deathstarbench/hotel-reservation:0.3.5
just samples load-image deathstarbench/hotel-reservation:0.3.5
```

Deploy and verify:

```sh
just samples deploy hotel-reservation
just samples smoke hotel-reservation
```

The first request after a deploy takes up to 15 s.

A search returns a GeoJSON `FeatureCollection` of hotels.

The Go services restart once or twice on the first deploy while MongoDB and consul start. A service still restarting after two minutes is a fault.

MongoDB runs on `emptyDir`, and each service seeds its collection only when it starts. After a node reboot or a MongoDB restart, the search answers `500` until the services restart too:

```sh
just samples restart hotel-reservation
```

---

# 8. otel-demo

The [OpenTelemetry Demo](https://github.com/open-telemetry/opentelemetry-demo) 3.1.0.

Every request asks for product recommendations:

```text
frontend (Node.js) → recommendation (Python) → product-catalog (Go) → PostgreSQL
                   → currency (C++)
```

The frontend asks for the 300m it is limited to. At 5.6 ms of CPU per request, one pod holds about 37 RPS at 70%, so a burst at `--scale 1` needs about four pods, while the services behind it stay under half a core each.

Deploy and verify:

```sh
just samples deploy otel-demo
just samples smoke otel-demo
```

A successful smoke test takes about 15 ms.

`k8s.yaml` is generated from the demo's `compose.yaml` and `.env` at tag 3.1.0, with these changes:

```text
the core services only
    the Envoy proxy, image provider, load generator, flagd UI and telemetry docs are left out;
    the ingress goes straight to the frontend

a collector that accepts every signal and drops it
    instead of Jaeger, Prometheus, Grafana and OpenSearch

memory limits of at least 64Mi
    compose sizes some services at 20M

enableServiceLinks: false on every pod
    Kubernetes otherwise sets FLAGD_PORT=tcp://…:8013, flagd reads it as its own port
    and listens on a random one, and every flag lookup from the Python services
    waits out a 500 ms deadline

the frontend's CPU request equals its 300m limit

a 512Mi memory limit on the frontend, not compose's 250M
    one pod queues the burst before the autoscaler adds another,
    and at 250Mi the queue killed it with OOMKilled
```

---

# 9. Run a burst

Run one burst with the HPA alone:

```sh
just samples burst hotel-reservation
```

Run it with croft, under the prefix `hr-x`, into `data/web/hr-x-croft-burst/`:

```sh
just samples burst hotel-reservation croft hr-x
```

A burst runs the `burst` profile at the application's `scale` from `apps.json`, with relaxed probes, pinned to `node-a`.

The nodes differ by up to 1.69 times in CPU speed, so every run of a comparison has to land on the same one.

Run the full comparison, the two arms alternated three times, then scored:

```sh
just samples series hotel-reservation hr
```

A series takes about 1 h 15 min per application.

---

# 10. Calibrate an application

Measure the `scaler` entry croft needs:

```sh
just samples calibrate otel-demo
```

Choose other levels and durations:

```sh
just samples calibrate otel-demo "10 20 35" 75
```

Reading and accepting the result is described in [RUNNING.md](../RUNNING.md#8-calibrate-an-application).

---

# 11. Recipes

| Recipe | What it does |
|---|---|
| `apps` | lists the applications, namespaces and request URLs |
| `load-image <image>` | loads a local Docker image into every node through `labs/k3s-cluster/load-image.yml` |
| `deploy <app>` | applies `k8s.yaml` and waits until every Deployment is Available |
| `status <app>` | the application's pods and their nodes |
| `smoke <app>` | one request on the experiment path: status code and time |
| `url <app>` | the request URL on the server and over Tailscale |
| `restart <app>` | restarts every Deployment of the application |
| `remove <app>` | deletes the application and its namespace |
| `burst <app> [arm] [prefix]` | one burst run; `arm` is `none` or `croft` |
| `series <app> <prefix>` | three repetitions of the two arms, then `compare` |
| `calibrate <app> [levels] [seconds]` | measures the `scaler` entry on one pinned pod at constant loads |
| `compare <prefix>...` | scores finished burst runs |
| `monitoring` | applies the Grafana dashboard and prints its URL |

---

# 12. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `smoke` prints `000` | The ingress has no route or the cluster is unreachable. Check `just samples status <app>` and `kubectl get ingress -A`. |
| `smoke` prints `500` on `hotel-reservation` | Its MongoDB lost its data. Run `just samples restart hotel-reservation`. |
| `smoke` takes about 520 ms on `otel-demo` | flagd listens on a random port. Check that every pod has `enableServiceLinks: false`. |
| `deploy` times out | An image is still pulling or a pod crashes. Run `just samples status <app>`, then `kubectl -n <namespace> describe pod <pod>`. |
| A run stops with `<image> is missing on <nodes>` | Load that image on every node with `load-image`. |

---

# 13. Switching applications

Applications can stay deployed side by side. An idle one uses little CPU.

Only one experiment runs at a time, so two deployed applications never take load together.

Remove one that is no longer needed:

```sh
just samples remove otel-demo
```

---

# 14. Add an application

Create `workloads/<name>/k8s.yaml`:

```text
[ ] its own namespace
[ ] an app: <name> label on every pod
[ ] an Ingress on <name>.127.0.0.1.sslip.io and <name>.100.64.0.10.sslip.io
[ ] equal CPU request and limit on the frontend container
[ ] readiness and liveness probes on the frontend container
```

Equal requests and limits, and probes, make the HPA's target and `--probes relaxed` mean the same as for the other applications.

Create `workloads/<name>/hpa.yaml` for the frontend Deployment, at 70% CPU and 1 to 10 replicas.

Add an entry to `apps.json`:

```text
namespace, deployment, container, manifests, host, path, health
```

Measure its calibration:

```sh
just samples calibrate <name>
```

Add the `scaler` entry it prints, and a `scale` that makes a burst need about four pods.
