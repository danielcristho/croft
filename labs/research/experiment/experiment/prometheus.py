"""Range queries for the resource and replica series a web run records next to its load."""

import csv
import json
import os
import urllib.parse
import urllib.request
from pathlib import Path

from . import kube
from .apps import App

# Consts
PROMETHEUS = os.environ.get("PROM_HOST", "http://prometheus.127.0.0.1.sslip.io:30080")
STEP_S = 5
# rate needs at least two samples per window; cAdvisor is scraped every 10 s, so 30 s drops pods at random.
WINDOW = "1m"
# The collector is scraped every 5 s, so 15 s holds three samples and keeps the signal's timing.
SIGNAL_WINDOW = "15s"
# Each collector series: its labels beyond the container, and whether it is a counter.
SIGNALS = {
    "syscall_rate": ("croft_container_syscall_rate", ("syscall",), False),
    "syscall_bytes_s": ("croft_container_syscall_bytes_total", ("syscall",), True),
    "socket_io_s": ("croft_container_socket_io_total", ("direction",), True),
    "connects_s": ("croft_container_connects_total", ("address", "port"), True),
    "cpu_cores": ("croft_container_cpu_cores", (), False),
    "runqueue_wait": ("croft_container_runqueue_wait", (), False),
}
SERVICE_COLUMNS = ["cpu_cores", "cpu_throttled_fraction", "memory_bytes", "restarts", "replicas", "ready_replicas"]


def queries(namespace: str, deployment: str) -> dict[str, str]:
    pods = f'namespace="{namespace}",pod=~"{deployment}-.*"'
    # The linkerd-proxy sidecar is left out, so CPU and throttling stay the application's own.
    containers = f'{pods},container!="",container!="POD",container!="linkerd-proxy"'
    return {
        "cpu_cores": f"sum(rate(container_cpu_usage_seconds_total{{{containers}}}[{WINDOW}]))",
        "memory_bytes": f"sum(container_memory_working_set_bytes{{{containers}}})",
        "network_rx_bytes_s": f"sum(rate(container_network_receive_bytes_total{{{pods}}}[{WINDOW}]))",
        "network_tx_bytes_s": f"sum(rate(container_network_transmit_bytes_total{{{pods}}}[{WINDOW}]))",
        # kube-prometheus-stack drops cfs_throttled_seconds_total; the share of throttled periods survives.
        "cpu_throttled_fraction": (
            f"sum(rate(container_cpu_cfs_throttled_periods_total{{{containers}}}[{WINDOW}]))"
            f" / sum(rate(container_cpu_cfs_periods_total{{{containers}}}[{WINDOW}]))"
        ),
        "restarts": f"sum(kube_pod_container_status_restarts_total{{{pods}}})",
        "replicas": f'kube_deployment_status_replicas{{namespace="{namespace}",deployment="{deployment}"}}',
        "ready_replicas": f'kube_deployment_status_replicas_available{{namespace="{namespace}",deployment="{deployment}"}}',
        "desired_replicas": f'kube_horizontalpodautoscaler_status_desired_replicas{{namespace="{namespace}"}}',
    }


def query_range(query: str, start_s: int, end_s: int) -> list[dict]:
    params = urllib.parse.urlencode({"query": query, "start": start_s, "end": end_s, "step": STEP_S})
    with urllib.request.urlopen(f"{PROMETHEUS}/api/v1/query_range?{params}", timeout=30) as response:
        return json.load(response)["data"]["result"]


def series(query: str, start_s: int, end_s: int) -> dict[int, float]:
    """One value per timestamp, summed over every stream the query returns."""
    return series_by(query, (), start_s, end_s).get((), {})


def series_by(query: str, labels: tuple[str, ...], start_s: int, end_s: int) -> dict[tuple[str, ...], dict[int, float]]:
    """Values per timestamp for each combination of labels, summed over the streams sharing it; NaN samples are dropped."""
    streams: dict[tuple[str, ...], dict[int, float]] = {}
    for stream in query_range(query, start_s, end_s):
        values = streams.setdefault(tuple(stream["metric"].get(label, "") for label in labels), {})
        for timestamp, value in stream["values"]:
            if value != "NaN":
                values[int(timestamp)] = values.get(int(timestamp), 0.0) + float(value)
    return streams


def export(path: Path, namespace: str, deployment: str, start_s: int, end_s: int) -> None:
    columns = {name: series(query, start_s, end_s) for name, query in queries(namespace, deployment).items()}
    timestamps = sorted(set().union(*columns.values()))
    with path.open("w", newline="") as out:
        writer = csv.writer(out)
        writer.writerow(["unix_s", *columns])
        for timestamp in timestamps:
            writer.writerow([timestamp, *(columns[name].get(timestamp, "") for name in columns)])


# signal_query joins on container_id because a collector series names the collector's pod. Zero rates are dropped,
# and the linkerd-proxy sidecar stays out, as in queries.
def signal_query(namespace: str, metric: str, labels: tuple[str, ...], counter: bool) -> str:
    source = f"rate({metric}[{SIGNAL_WINDOW}])" if counter else metric
    pods = f'kube_pod_container_info{{namespace="{namespace}",container!="linkerd-proxy"}}'
    ids = f'label_replace({pods}, "container_id", "$1", "container_id", ".*://(.+)")'
    joined = f"{source} * on (container_id) group_left (pod) {ids}"
    by = ", ".join(("service", *labels))
    return f'sum by ({by}) (label_replace({joined}, "service", "$1", "pod", "(.+)-[^-]+-[^-]+")) > 0'


# export_signals records the collector's series for every service, in long form since their labels differ.
def export_signals(path: Path, namespace: str, start_s: int, end_s: int) -> None:
    rows = []
    for signal, (metric, labels, counter) in SIGNALS.items():
        query = signal_query(namespace, metric, labels, counter)
        for (service, *key), values in series_by(query, ("service", *labels), start_s, end_s).items():
            rows.extend((timestamp, service, signal, ":".join(key), value) for timestamp, value in values.items())
    with path.open("w", newline="") as out:
        writer = csv.writer(out)
        writer.writerow(["unix_s", "service", "signal", "key", "value"])
        writer.writerows(sorted(rows))


# export_services records every deployment of the namespace with its role and language from apps.json, so a bottleneck
# moving off the frontend shows. A deployment apps.json does not list gets empty labels.
def export_services(path: Path, app: App, start_s: int, end_s: int) -> None:
    deployments = sorted(item["metadata"]["name"] for item in kube.get("deployments", app.namespace)["items"])
    with path.open("w", newline="") as out:
        writer = csv.writer(out)
        writer.writerow(["unix_s", "service", "role", "language", *SERVICE_COLUMNS])
        for deployment in deployments:
            service = app.services.get(deployment)
            labels = (service.role, service.language) if service else ("", "")
            found = queries(app.namespace, deployment)
            columns = {name: series(found[name], start_s, end_s) for name in SERVICE_COLUMNS}
            for timestamp in sorted(set().union(*columns.values())):
                writer.writerow([timestamp, deployment, *labels, *(columns[name].get(timestamp, "") for name in SERVICE_COLUMNS)])
