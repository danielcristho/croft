"""The call graph of a Linkerd-meshed namespace for a run's window: rate, success and latency per edge."""

import csv
import subprocess
from pathlib import Path

from . import kube, prometheus
from .apps import ROOT, App

# Consts
INJECT = "linkerd.io/inject"
PROXY = "linkerd-proxy"
# The proxies are scraped every 10 s (workloads/linkerd/podmonitor.yaml); 30 s holds three samples.
WINDOW = "30s"
# Ingress-nginx is not meshed, so the frontend's inbound traffic has no source deployment; it is named after its origin.
INGRESS = "ingress"
EDGE_COLUMNS = ["rps", "success_rate", "p50_ms", "p95_ms"]


# has_proxy looks in initContainers too, where Linkerd puts the proxy as a native sidecar.
def has_proxy(pod: dict) -> bool:
    spec = pod["spec"]
    return any(c["name"] == PROXY for c in spec.get("containers", []) + spec.get("initContainers", []))


def needs_restart(pods: list[dict], mesh: bool) -> bool:
    return any(has_proxy(pod) != mesh for pod in pods)


# set_mesh annotates the namespace or removes the annotation, and restarts the app when a running pod's proxy disagrees.
# kubectl apply keeps the annotation, because it is not in the manifest's last-applied configuration. `just restart`
# holds the restart order: registries and stores before the services that need them.
def set_mesh(app: App, mesh: bool) -> None:
    kube.kubectl("annotate", "namespace", app.namespace, f"{INJECT}=enabled" if mesh else f"{INJECT}-", "--overwrite")
    running = [pod for pod in kube.get("pods", app.namespace)["items"] if pod["status"].get("phase") == "Running"]
    if needs_restart(running, mesh):
        subprocess.run(["just", "restart", app.name], cwd=ROOT, check=True)


def edge_queries(selector: str, labels: tuple[str, ...]) -> dict[str, str]:
    by = ", ".join(labels)
    total = f"sum by ({by}) (rate(response_total{{{selector}}}[{WINDOW}]))"
    success = f'sum by ({by}) (rate(response_total{{{selector},classification="success"}}[{WINDOW}]))'

    def quantile(q: float) -> str:
        return f"histogram_quantile({q}, sum by (le, {by}) (rate(response_latency_ms_bucket{{{selector}}}[{WINDOW}])))"

    return {"rps": total, "success_rate": f"{success} / {total}", "p50_ms": quantile(0.5), "p95_ms": quantile(0.95)}


# edges reads every meshed call in the namespace from the caller's outbound proxy, and the entry deployment's
# inbound proxy for the requests arriving from the ingress.
def edges(namespace: str, entry: str, start_s: int, end_s: int) -> dict[tuple[str, str], dict[str, dict[int, float]]]:
    outbound = (f'namespace="{namespace}",direction="outbound"', ("deployment", "dst_deployment"))
    inbound = (f'namespace="{namespace}",direction="inbound",deployment="{entry}"', ("deployment",))
    found: dict[tuple[str, str], dict[str, dict[int, float]]] = {}
    for selector, labels in (outbound, inbound):
        for column, query in edge_queries(selector, labels).items():
            for key, values in prometheus.series_by(query, labels, start_s, end_s).items():
                edge = key if len(key) == 2 else (INGRESS, key[0])
                found.setdefault(edge, {})[column] = values
    return found


def write_edges(path: Path, found: dict[tuple[str, str], dict[str, dict[int, float]]]) -> None:
    with path.open("w", newline="") as out:
        writer = csv.writer(out)
        writer.writerow(["unix_s", "src", "dst", *EDGE_COLUMNS])
        rows = []
        for (src, dst), columns in found.items():
            for timestamp in sorted(set().union(*columns.values())):
                rows.append([timestamp, src, dst, *(columns.get(name, {}).get(timestamp, "") for name in EDGE_COLUMNS)])
        writer.writerows(sorted(rows, key=lambda row: (row[0], row[1], row[2])))


def export_edges(path: Path, namespace: str, entry: str, start_s: int, end_s: int) -> None:
    write_edges(path, edges(namespace, entry, start_s, end_s))

