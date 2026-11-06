"""Range queries for the resource and replica series a web run records next to its load."""

import csv
import json
import os
import urllib.parse
import urllib.request
from pathlib import Path

# Consts
PROMETHEUS = os.environ.get("PROM_HOST", "http://prometheus.127.0.0.1.sslip.io:30080")
STEP_S = 5
# rate needs at least two samples per window; cAdvisor is scraped every 10 s, so 30 s drops pods at random.
WINDOW = "1m"


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
