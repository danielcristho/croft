# Sample applications

The burst experiments run against three sample applications. Each sample is a folder here with a `k8s.yaml` for the application and an `hpa.yaml` for its baseline autoscaler, and an entry in [`apps.json`](../apps.json) that tells the harness where it runs and which request to send.

Every recipe below runs from the repository root as `just samples <recipe>`, or from `labs/research/experiment` as `just <recipe>`. [RUNNING.md](../RUNNING.md) covers the experiments themselves.

**Table of contents**

- [Samples](#samples)
- [Prerequisites](#prerequisites)
- [Deploying a sample](#deploying-a-sample)
  - [webapp](#webapp)
  - [hotel-reservation](#hotel-reservation)
  - [otel-demo](#otel-demo)
- [Running a burst](#running-a-burst)
- [Recipes](#recipes)
- [Troubleshooting](#troubleshooting)
- [Adding a sample](#adding-a-sample)

## Samples

| Sample | Source | Request path | Services | `--scale` |
|---|---|---|---:|---:|
| `webapp` | this repository | `/cpu`: 40 to 65 ms of CPU in one Python process | 1 | 0.5 |
| `hotel-reservation` | [DeathStarBench](https://github.com/delimitrou/DeathStarBench) hotelReservation, image 0.3.5 | `/hotels`: frontend, search, geo, rate, reservation, profile (Go, gRPC) | 19 | 2 |
| `otel-demo` | [OpenTelemetry Demo](https://github.com/open-telemetry/opentelemetry-demo) 3.1.0 | `/api/recommendations`: frontend (Node.js), recommendation (Python), product-catalog (Go, PostgreSQL), currency (C++) | 15 | 1 |

The HPA, KEDA and croft scale each sample's frontend; the services behind it stay at one replica.

## Prerequisites

- The lab cluster with ingress-nginx on NodePort 30080. See [`labs/k3s-cluster`](../../../k3s-cluster).
- [`just`](https://github.com/casey/just), `kubectl`, `curl` and Python 3.
- `KUBECONFIG` pointing at the cluster. The recipes default to `~/.kube/croft-k3s.yaml`.

```bash
just samples apps        # the samples, their namespaces and request URLs
```

## Deploying a sample

The harness redeploys a sample at the start of every run. Deploy it by hand once, before the first run, to pull its images and see that it answers.

### webapp

Its image is in no registry, so build it and load it on every node first:

```bash
just samples build-webapp
just samples load-image croft-webapp:v1      # asks for the sudo password
just samples deploy webapp
just samples smoke webapp                    # 200
```

### hotel-reservation

```bash
just samples deploy hotel-reservation
just samples smoke hotel-reservation         # 200; the first request after a deploy takes up to 15 s
```

The harness checks that the first image of `k8s.yaml` is on every node, and the 1.2 GB `deathstarbench/hotel-reservation:0.3.5` is only pulled where a pod lands. Load it everywhere once:

```bash
docker pull deathstarbench/hotel-reservation:0.3.5
just samples load-image deathstarbench/hotel-reservation:0.3.5
```

> **Note**
> MongoDB runs on `emptyDir`, and each service seeds its collection only when it starts. After a node reboot or a MongoDB restart the search answers 500 until the services restart too: `just samples restart hotel-reservation`.

### otel-demo

```bash
just samples deploy otel-demo
just samples smoke otel-demo                 # 200 in about 15 ms
```

`k8s.yaml` is generated from the demo's `compose.yaml` and `.env` at tag 3.1.0, with these changes:

- The core services only. The Envoy proxy, image provider, load generator, flagd UI and telemetry docs are left out; the ingress goes straight to the frontend.
- A collector that accepts every signal and drops it, instead of Jaeger, Prometheus, Grafana and OpenSearch.
- Memory limits of at least 64Mi; compose sizes some services at 20M.
- `enableServiceLinks: false` on every pod. Kubernetes otherwise sets `FLAGD_PORT=tcp://…:8013`, flagd reads it as its own port and listens on a random one, and every flag lookup from the Python services waits out a 500 ms deadline.
- The frontend asks for the 300m it is limited to. At its 4.7 ms of CPU per request one pod holds about 44 RPS at 70%, so a burst at `--scale 1` needs about four pods, while the services behind it stay under half a core each.

## Running a burst

```bash
just samples burst hotel-reservation               # the HPA alone
just samples burst hotel-reservation keda          # KEDA
just samples burst hotel-reservation croft hr-x    # croft, under the prefix hr-x
```

A burst runs the `burst` profile at the sample's `scale` from `apps.json`, with relaxed probes, pinned to `node-a`. The nodes differ by up to 1.69 times in CPU speed, so every run of a comparison has to land on the same one.

A full comparison alternates the three arms three times, about 1 h 50 min per sample, then scores them:

```bash
just samples series hotel-reservation hr
```

## Recipes

| Recipe | What it does |
|---|---|
| `apps` | lists the samples, namespaces and request URLs |
| `build-webapp` | builds `croft-webapp:v1` |
| `load-image <image>` | loads a local Docker image into every node through `labs/k3s-cluster/load-image.yml` |
| `deploy <app>` | applies `k8s.yaml` and waits until every Deployment is Available |
| `status <app>` | the sample's pods and their nodes |
| `smoke <app>` | one request on the experiment path: status code and time |
| `url <app>` | the request URL on the LAN and over Tailscale |
| `restart <app>` | restarts every Deployment of the sample |
| `remove <app>` | deletes the sample and its namespace |
| `burst <app> [arm] [prefix]` | one burst run; `arm` is `none`, `keda` or `croft` |
| `series <app> <prefix>` | three repetitions of the three arms, then `compare` |
| `calibrate <app> [levels] [seconds]` | measures the `scaler` entry on one pinned pod at constant loads |
| `compare <prefix>...` | scores finished burst runs |

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `smoke` prints `000` | The ingress has no route or the cluster is unreachable. Check `just samples status <app>` and `kubectl get ingress -A`. |
| `smoke` prints `500` on `hotel-reservation` | Its MongoDB lost its data. Run `just samples restart hotel-reservation`. |
| `smoke` takes about 520 ms on `otel-demo` | flagd listens on a random port. Check that every pod has `enableServiceLinks: false`. |
| `deploy` times out | An image is still pulling or a pod crashes. `just samples status <app>`, then `kubectl -n <namespace> describe pod <pod>`. |
| A run stops with `<image> is missing on <nodes>` | Load that image on every node with `load-image`. |

## Adding a sample

1. Create `workloads/<name>/k8s.yaml` in its own namespace, with an `app: <name>` label on every pod and an Ingress on `<name>.127.0.0.1.sslip.io` and `<name>.100.64.0.10.sslip.io`.
2. Create `workloads/<name>/hpa.yaml` for the Deployment the experiments scale, at 70% CPU and 1 to 10 replicas.
3. Give that Deployment's container equal CPU requests and limits, and probes, so the HPA's target and `--probes relaxed` mean the same as for the other samples.
4. Add an entry to `apps.json`: `namespace`, `deployment`, `container`, `manifests`, `host`, `path`, `health`.
5. Measure its calibration with `just samples calibrate <name>`, as [RUNNING.md](../RUNNING.md#measuring-a-calibration) describes, and add it and a `scale` to the entry.
