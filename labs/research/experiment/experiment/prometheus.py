"""Range queries for the resource and replica series the webapp suite records next to Locust."""

import csv
import json
import os
import urllib.parse
import urllib.request
from pathlib import Path

# Consts
PROMETHEUS = os.environ.get("PROM_HOST", "http://prometheus.127.0.0.1.sslip.io:30080")
STEP_S = 5
WINDOW = "30s"


def queries(namespace: str, deployment: str) -> dict[str, str]:
    pods = f'namespace="{namespace}",pod=~"{deployment}-.*"'
    containers = f'{pods},container!="",container!="POD"'
    return {
        "cpu_cores": f"sum(rate(container_cpu_usage_seconds_total{{{containers}}}[{WINDOW}]))",
        "memory_bytes": f"sum(container_memory_working_set_bytes{{{containers}}})",
        "network_rx_bytes_s": f"sum(rate(container_network_receive_bytes_total{{{pods}}}[{WINDOW}]))",
        "network_tx_bytes_s": f"sum(rate(container_network_transmit_bytes_total{{{pods}}}[{WINDOW}]))",
        "replicas": f'kube_deployment_status_replicas{{namespace="{namespace}",deployment="{deployment}"}}',
        "ready_replicas": f'kube_deployment_status_replicas_available{{namespace="{namespace}",deployment="{deployment}"}}',
        "desired_replicas": f'kube_horizontalpodautoscaler_status_desired_replicas{{namespace="{namespace}"}}',
    }


def series(query: str, start_s: int, end_s: int) -> dict[int, float]:
    params = urllib.parse.urlencode({"query": query, "start": start_s, "end": end_s, "step": STEP_S})
    with urllib.request.urlopen(f"{PROMETHEUS}/api/v1/query_range?{params}", timeout=30) as response:
        result = json.load(response)["data"]["result"]
    values: dict[int, float] = {}
    for stream in result:
        for timestamp, value in stream["values"]:
            values[int(timestamp)] = values.get(int(timestamp), 0.0) + float(value)
    return values


def export(path: Path, namespace: str, deployment: str, start_s: int, end_s: int) -> None:
    columns = {name: series(query, start_s, end_s) for name, query in queries(namespace, deployment).items()}
    timestamps = sorted(set().union(*columns.values()))
    with path.open("w", newline="") as out:
        writer = csv.writer(out)
        writer.writerow(["unix_s", *columns])
        for timestamp in timestamps:
            writer.writerow([timestamp, *(columns[name].get(timestamp, "") for name in columns)])
