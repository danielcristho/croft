# croft for Python

The collector's signals as pandas frames, for notebooks and analysis.

```sh
pip install "croft-signals[plot]"
uv add "croft-signals[plot]"
uv add --editable ../python    # from a project inside this repository
```

## 1. Sources

| Source | Function | History | Pods |
|---|---|---|---|
| Prometheus | `metrics.signal()` and one function per signal | as long as Prometheus keeps it | joined through kube-state-metrics |
| collector `/metrics` | `metrics.live()` | the newest poll | `container_id` |
| dataset files | `dataset.signal()` and one function per signal; `dataset.read()` for raw rows | as long as the node keeps it | `container_id`, or names from `kube.names()` |

`CROFT_PROMETHEUS` sets the Prometheus URL (`PROM_HOST` is read as a fallback),
`CROFT_METRICS_ADDR` the collector address and `CROFT_DATASET` the dataset directory.

## 2. Signals

| Signal | Metric | Unit |
|---|---|---|
| `syscall_rate` | `croft_container_syscall_rate` | calls/s, per `syscall` |
| `syscall_bytes` | `croft_container_syscall_bytes_total` | bytes/s, per `syscall` |
| `socket_io` | `croft_container_socket_io_total` | calls/s, per `direction` |
| `connects` | `croft_container_connects_total` | connects/s, per `address` and `port` |
| `cpu_cores` | `croft_container_cpu_cores` | cores |
| `cpu_throttled_fraction` | `croft_container_cpu_throttled_fraction` | fraction |
| `cpu_pressure` | `croft_container_cpu_pressure_avg10` | % |
| `runqueue_wait` | `croft_container_runqueue_wait` | s/s |
| `inflight_requests` | `croft_container_inflight_requests` | requests |

Counters come as rates over 15 s or the step, whichever is longer; gauges are averaged over each step longer than
5 s. `metrics.catalog()` returns this table. The dataset holds `syscall_rate`, `runqueue_wait` and the three CPU
signals; the other three are on `/metrics` and Prometheus.

## 3. Usage

```python
from croft import metrics, plot

frame = metrics.syscall_rate("boutique", "frontend", since="1h", match={"syscall": "getpeername"})
plot.lines(frame, by="pod")

metrics.runqueue_wait("hotel-res", since="2026-11-18 09:00", until="2026-11-18 10:00")
metrics.wide(frame, by="deployment")       # one column per deployment
metrics.query("sum(rate(container_cpu_usage_seconds_total[1m]))", since="30m")
metrics.live("127.0.0.1:9464")             # newest poll of one collector
```

Without Prometheus, the same frames come from the dataset files, with `node` and `container_id` added:

```python
from croft import dataset, kube

names = kube.names()                       # container_id to namespace, pod and container, through kubectl
frame = dataset.syscall_rate("boutique", "frontend", since="1h", names=names, match={"syscall": "getpeername"})
```

A summed signal (`syscall_rate`, `runqueue_wait`) is the container's total per poll, averaged over the polls of
each step; a poll without a row for the container counts as 0, since idle processes are written every 15 polls.
A CPU signal is averaged over the polls that hold it. `kube.names()` knows the pods the API server holds, so
containers of deleted pods keep their `container_id` only.

Each row holds `time` (UTC), `namespace`, `deployment`, `pod`, `container`, the
signal's own labels and `value`. `deployment` comes from the pod name and is empty
for pods outside a Deployment. `since` and `until` take a duration before now
(`"90m"`), a date or a unix time; the step keeps a range under 1000 samples.

## 4. Tests

```sh
uv run python -m unittest discover -s tests
```
