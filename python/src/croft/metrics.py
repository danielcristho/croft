"""Collector signals from Prometheus as pandas frames, one row per sample, joined to the pod and container that produced it."""

import os
import re
import time
import urllib.request
from dataclasses import dataclass
from datetime import datetime

import pandas as pd

from .prometheus import Prometheus


@dataclass(frozen=True)
class Signal:
    metric: str
    labels: tuple[str, ...]
    counter: bool
    unit: str


SIGNALS = {
    "syscall_rate": Signal("croft_container_syscall_rate", ("syscall",), False, "calls/s"),
    "syscall_bytes": Signal("croft_container_syscall_bytes_total", ("syscall",), True, "bytes/s"),
    "socket_io": Signal("croft_container_socket_io_total", ("direction",), True, "calls/s"),
    "connects": Signal("croft_container_connects_total", ("address", "port"), True, "connects/s"),
    "cpu_cores": Signal("croft_container_cpu_cores", (), False, "cores"),
    "cpu_throttled_fraction": Signal("croft_container_cpu_throttled_fraction", (), False, "fraction"),
    "cpu_pressure": Signal("croft_container_cpu_pressure_avg10", (), False, "%"),
    "runqueue_wait": Signal("croft_container_runqueue_wait", (), False, "s/s"),
}
# The collector is scraped every 5 s, so 15 s holds three samples for rate().
RATE_WINDOW_S = 15
MIN_STEP_S = 5
MAX_POINTS = 1000
DURATION = re.compile(r"(\d+(?:\.\d+)?)([smhdw])")
SECONDS = {"s": 1, "m": 60, "h": 3600, "d": 86400, "w": 604800}
POD = re.compile(r"(.+)-[a-z0-9]{5,10}-[a-z0-9]{5}")
KUBE_LABELS = ("namespace", "pod", "container")
LINE = re.compile(r"([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})? (\S+)")
LABEL = re.compile(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"')

Time = str | float | int | datetime | pd.Timestamp


def catalog() -> pd.DataFrame:
    """Every signal with its Prometheus metric, extra labels, kind and unit."""
    return pd.DataFrame(
        [
            {"signal": name, "metric": s.metric, "labels": ",".join(s.labels), "counter": s.counter, "unit": s.unit}
            for name, s in SIGNALS.items()
        ]
    ).set_index("signal")


def signal(
    name: str,
    namespace: str | None = None,
    deployment: str | None = None,
    *,
    since: Time = "1h",
    until: Time | None = None,
    step_s: float | None = None,
    match: dict[str, str] | None = None,
    kube: bool = True,
    prometheus: Prometheus | None = None,
) -> pd.DataFrame:
    """Samples of one signal between since and until; counters come as per-second rates.

    namespace and deployment narrow the pods; match narrows the signal's own labels, such as {"syscall": "sendto"}.
    kube=False skips the join to kube-state-metrics and keys rows by container_id, for containers outside Kubernetes.
    """
    spec = SIGNALS[name]
    start_s, end_s, step_s = window(since, until, step_s)
    query = promql(spec, namespace, deployment, match, kube, step_s)
    streams = (prometheus or Prometheus()).query_range(query, start_s, end_s, step_s)
    frame = frame_of(streams, spec, kube)
    frame.attrs.update(signal=name, unit=spec.unit, query=query)
    return frame


# promql averages a gauge over each step and widens a counter's rate window to it, so a long range does not
# sample one scrape per step and alias periodic spikes.
def promql(
    spec: Signal,
    namespace: str | None,
    deployment: str | None,
    match: dict[str, str] | None,
    kube: bool,
    step_s: float = MIN_STEP_S,
) -> str:
    unknown = set(match or {}) - set(spec.labels)
    if unknown:
        raise ValueError(f"{spec.metric} has no label {', '.join(sorted(unknown))}; it has {spec.labels or 'none'}")
    expr = f"{spec.metric}{_selector(match or {})}"
    span = f"{max(RATE_WINDOW_S, step_s) if spec.counter else step_s:g}s"
    if spec.counter:
        expr = f"rate({expr}[{span}])"
    elif step_s > MIN_STEP_S:
        expr = f"avg_over_time({expr}[{span}])"
    if not kube:
        return expr
    pods = {"namespace": namespace} if namespace else {}
    if deployment:
        pods["pod=~"] = f"{re.escape(deployment)}-[a-z0-9]+-[a-z0-9]+"
    info = f'label_replace(kube_pod_container_info{_selector(pods)}, "container_id", "$1", "container_id", "[a-z-]+://(.+)")'
    labels = ", ".join(KUBE_LABELS)
    return f"{expr} * on (container_id) group_left ({labels}) max by (container_id, {labels}) ({info})"


def frame_of(streams: list[dict], spec: Signal, kube: bool) -> pd.DataFrame:
    keys = (*KUBE_LABELS, *spec.labels) if kube else ("container_id", *spec.labels)
    rows = []
    for stream in streams:
        labels = {key: stream["metric"].get(key) for key in keys}
        if kube:
            labels["deployment"] = deployment_of(labels["pod"])
        rows += [{"time": float(t), **labels, "value": float(v)} for t, v in stream["values"] if v != "NaN"]
    columns = ["time", *(("namespace", "deployment", "pod", "container") if kube else ("container_id",)), *spec.labels]
    frame = pd.DataFrame(rows, columns=[*columns, "value"])
    frame["time"] = pd.to_datetime(frame["time"], unit="s", utc=True)
    return frame.sort_values([*columns[1:], "time"], ignore_index=True)


def wide(frame: pd.DataFrame, by: str | list[str] = "pod") -> pd.DataFrame:
    """One column per value of by, summed over the other labels, ready for .plot()."""
    table = frame.pivot_table(index="time", columns=by, values="value", aggfunc="sum")
    table.attrs = dict(frame.attrs)
    return table


def live(address: str | None = None, timeout_s: float = 10) -> pd.DataFrame:
    """The newest poll of one collector, read from its /metrics: one row per series, counters as running totals.

    address is host:port or a URL, defaulting to CROFT_METRICS_ADDR. Rows are keyed by container_id; the collector
    holds no history and no pod names, so time ranges and the pod join need signal() and Prometheus.
    """
    address = address or os.environ.get("CROFT_METRICS_ADDR") or "127.0.0.1:9464"
    url = address if "://" in address else f"http://{address}"
    with urllib.request.urlopen(f"{url.rstrip('/')}/metrics", timeout=timeout_s) as response:
        text = response.read().decode()
    names = {spec.metric: name for name, spec in SIGNALS.items()}
    rows = []
    for line in text.splitlines():
        sample = _sample(line)
        if sample and sample[0] in names:
            metric, labels, value = sample
            rows.append({"signal": names[metric], **labels, "value": value})
    frame = pd.DataFrame(rows, columns=None if rows else ["signal", "container_id", "value"])
    poll = re.search(r"^croft_poll_unix_ms (\d+)", text, re.MULTILINE)
    frame.attrs.update(poll_time=pd.Timestamp(int(poll.group(1)), unit="ms", tz="UTC") if poll else None)
    return frame


def _sample(line: str) -> tuple[str, dict[str, str], float] | None:
    found = LINE.fullmatch(line.strip())
    if not found or line.startswith("#"):
        return None
    labels = {
        key: value.replace('\\"', '"').replace("\\\\", "\\") for key, value in LABEL.findall(found.group(2) or "")
    }
    return found.group(1), labels, float(found.group(3))


def window(since: Time, until: Time | None, step_s: float | None) -> tuple[float, float, float]:
    """Start, end and step in unix seconds; the step keeps a range under MAX_POINTS samples."""
    now = time.time()
    end_s = _seconds(until, now) if until is not None else now
    start_s = _seconds(since, end_s)
    if start_s >= end_s:
        raise ValueError(f"since ({since}) is not before until ({until or 'now'})")
    return start_s, end_s, step_s or max(MIN_STEP_S, (end_s - start_s) / MAX_POINTS)


def deployment_of(pod: str | None) -> str | None:
    found = POD.fullmatch(pod or "")
    return found.group(1) if found else None


def query(
    text: str,
    *,
    since: Time = "1h",
    until: Time | None = None,
    step_s: float | None = None,
    prometheus: Prometheus | None = None,
) -> pd.DataFrame:
    """Any PromQL range query as a long frame with one column per label."""
    rows = [
        {"time": float(t), **stream["metric"], "value": float(v)}
        for stream in (prometheus or Prometheus()).query_range(text, *window(since, until, step_s))
        for t, v in stream["values"]
        if v != "NaN"
    ]
    frame = pd.DataFrame(rows, columns=None if rows else ["time", "value"])
    frame["time"] = pd.to_datetime(frame["time"], unit="s", utc=True)
    frame.attrs.update(query=text)
    return frame


def syscall_rate(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("syscall_rate", namespace, deployment, **options)


def syscall_bytes(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("syscall_bytes", namespace, deployment, **options)


def socket_io(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("socket_io", namespace, deployment, **options)


def connects(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("connects", namespace, deployment, **options)


def cpu_cores(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_cores", namespace, deployment, **options)


def cpu_throttled_fraction(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_throttled_fraction", namespace, deployment, **options)


def cpu_pressure(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_pressure", namespace, deployment, **options)


def runqueue_wait(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("runqueue_wait", namespace, deployment, **options)


def _selector(labels: dict[str, str]) -> str:
    if not labels:
        return ""
    pairs = []
    for key, value in labels.items():
        name, op = (key[:-2], "=~") if key.endswith("=~") else (key, "=")
        pairs.append(f"{name}{op}{_quote(value)}")
    return "{" + ",".join(pairs) + "}"


def _quote(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


# _seconds reads a unix time, a datetime, or a duration such as "90m" before relative_to.
def _seconds(at: Time, relative_to: float | None = None) -> float:
    if isinstance(at, (int, float)):
        return float(at)
    if isinstance(at, datetime):
        return pd.Timestamp(at).timestamp()
    if relative_to is not None:
        parts = DURATION.findall(at)
        if parts and "".join(n + u for n, u in parts) == at:
            return relative_to - sum(float(n) * SECONDS[u] for n, u in parts)
    return pd.Timestamp(at).timestamp()
