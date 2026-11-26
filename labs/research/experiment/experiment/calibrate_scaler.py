"""croft scale's calibration for one app: syscalls and CPU per request on one pinned pod at fixed loads, from the collector."""

import csv
import json
import re
import statistics
import threading
import time
import urllib.request
from pathlib import Path

from . import kube, records, rps, scaler, web
from .apps import App
from .profiles import Profile

# Consts
OUT = records.DATA / "calibration"
SETTLE_S = 20
SAMPLE_S = 2
TARGET = 0.7
MIN_CALLS = 0.5
MIN_ANSWERED = 0.95
MAX_CPU_SHARE = 0.85
COLLECTOR_PORT = 9464


# collector_url is the /metrics address of the croft collector on a node.
def collector_url(node: str) -> str:
    pods = kube.get("pods", scaler.NAMESPACE, "-l", f"app={scaler.COLLECTORS}", "--field-selector", f"spec.nodeName={node}")["items"]
    if not pods:
        raise SystemExit(f"no croft collector runs on {node}")
    return f"http://{pods[0]['status']['podIP']}:{COLLECTOR_PORT}/metrics"


# sample reads one container's syscall rates, CPU cores and throttled share from the collector.
def sample(url: str, container_id: str) -> dict[str, float]:
    return parse_sample(urllib.request.urlopen(url, timeout=5).read().decode(), container_id)


# parse_sample keeps croft_container_syscall_rate only, since syscall_bytes_total carries a syscall label too.
def parse_sample(text: str, container_id: str) -> dict[str, float]:
    found = {}
    for line in text.splitlines():
        if container_id not in line:
            continue
        name, _, value = line.rpartition(" ")
        if name.startswith("croft_container_syscall_rate{") and (match := re.search(r'syscall="([^"]+)"', name)):
            found[match.group(1)] = float(value)
        elif name.startswith("croft_container_cpu_cores"):
            found["cpu_cores"] = float(value)
        elif name.startswith("croft_container_cpu_throttled_fraction"):
            found["throttled"] = float(value)
    return found


# top maps each pod of the namespace to its CPU in millicores, as metrics-server last saw it.
def top(namespace: str) -> dict[str, int]:
    usage = {}
    for line in kube.kubectl("top", "pods", "-n", namespace, "--no-headers").splitlines():
        pod, cpu, *_ = line.split()
        usage[pod.rsplit("-", 2)[0]] = int(cpu.rstrip("m"))
    return usage


# load_stats reads the client side of one level from its workload.csv, after the settle time. achieved_rps is what the
# generator sent; answered_rps counts the successful answers, which is what the pod's syscalls and CPU served.
def load_stats(path: Path) -> dict:
    rows = [r for r in csv.DictReader(path.open()) if float(r["elapsed_s"]) >= SETTLE_S]
    p95 = sorted(float(r["p95_latency_ms"]) for r in rows)
    seconds = float(rows[-1]["elapsed_s"]) - float(rows[0]["elapsed_s"]) + 1 if rows else 0
    return {
        "achieved_rps": round(statistics.mean(float(r["achieved_rps"]) for r in rows), 1),
        "answered_rps": round(sum(int(r["success"]) for r in rows) / seconds, 1) if seconds else 0.0,
        "failed": sum(int(r["failed"]) for r in rows),
        "p95_ms": p95[len(p95) // 2],
    }


# saturated says why a level cannot calibrate: failed or unanswered requests, or a pod at its CPU limit; None when it can.
def saturated(level: dict, limit_cores: float) -> str | None:
    if level["failed"]:
        return f"{level['failed']} failed requests"
    if level["answered_rps"] < MIN_ANSWERED * level["achieved_rps"]:
        return f"answered {level['answered_rps']} of {level['achieved_rps']} RPS sent"
    share = level["metrics"].get("cpu_cores", 0.0) / limit_cores
    if share >= MAX_CPU_SHARE:
        return f"CPU at {share:.0%} of the {limit_cores:g} core limit"
    return None


def summarize(levels: dict[int, dict]) -> dict:
    """The syscall whose calls per request move least across the levels, and the mean CPU per request."""
    per_request = {}
    for level in levels.values():
        rate = level["answered_rps"]
        for name, value in level["metrics"].items():
            if name not in ("cpu_cores", "throttled"):
                per_request.setdefault(name, []).append(value / rate)
    stable = {
        name: values for name, values in per_request.items()
        if len(values) == len(levels) and min(values) >= MIN_CALLS
    }
    spread = {name: (max(v) - min(v)) / statistics.mean(v) for name, v in stable.items()}
    best = min(spread, key=spread.get)
    cpu_ms = [level["metrics"]["cpu_cores"] / level["answered_rps"] * 1000 for level in levels.values()]
    return {
        "syscall": best,
        "calls_per_request": round(statistics.mean(stable[best]), 2),
        "cpu_per_request_ms": round(statistics.mean(cpu_ms), 2),
        "spread": {name: round(spread[name], 3) for name in sorted(spread, key=spread.get)[:5]},
    }


def run(app: App, levels: list[int], seconds: int, node: str | None, mix: bool = False) -> None:
    endpoints = app.endpoints(mix)
    name = f"{app.name}-mix" if mix else app.name
    web.deploy(app, 1, node)
    time.sleep(SETTLE_S)
    pods = [p for p in kube.get("pods", app.namespace, "-l", app.selector)["items"]
            if p["status"].get("phase") == "Running" and not p["metadata"].get("deletionTimestamp")]
    pod = pods[0]
    container_id = kube.container_id(next(s for s in pod["status"]["containerStatuses"] if s["name"] == app.container))
    url = collector_url(pod["spec"]["nodeName"])
    limit = scaler.limit_cores(app)
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"{app.name}: {pod['metadata']['name']} on {pod['spec']['nodeName']}, CPU limit {limit:g}")

    results = {}
    for level in levels:
        csv_path = OUT / f"{name}-{level}rps.csv"
        load = threading.Thread(target=rps.run, args=(app.host, endpoints, Profile("constant", seconds, rps=level), csv_path))
        load.start()
        time.sleep(SETTLE_S)
        samples = []
        while load.is_alive():
            samples.append(sample(url, container_id))
            time.sleep(SAMPLE_S)
        load.join()
        busiest = top(app.namespace)
        keys = set().union(*samples)
        metrics = {k: statistics.mean(s.get(k, 0.0) for s in samples) for k in keys}
        results[level] = {**load_stats(csv_path), "metrics": metrics, "pods_mcpu": busiest}
        r = results[level]
        per_request = metrics.get("cpu_cores", 0) / r["answered_rps"] * 1000 if r["answered_rps"] else float("nan")
        print(f"  {level:>4} RPS: sent {r['achieved_rps']}, answered {r['answered_rps']}, failed {r['failed']}, p95 {r['p95_ms']:.0f} ms, "
              f"CPU {per_request:.2f} ms/request, throttled {metrics.get('throttled', 0):.0%}")
        print("        busiest pods:", ", ".join(f"{p} {m}m" for p, m in sorted(busiest.items(), key=lambda x: -x[1])[:4]))
        if reason := saturated(r, limit):
            path = OUT / f"{name}-saturated.json"
            path.write_text(json.dumps({"app": app.name, "node": pod["spec"]["nodeName"], "limit_cores": limit,
                                        "mix": mix, "levels": results}, indent=1))
            raise SystemExit(f"{level} RPS saturates one pod ({reason}); rerun with levels below it. Levels so far in {path}")
        time.sleep(10)

    summary = summarize(results)
    capacity = TARGET * limit * 1000 / summary["cpu_per_request_ms"]
    path = OUT / f"{name}.json"
    path.write_text(json.dumps({"app": app.name, "node": pod["spec"]["nodeName"], "limit_cores": limit,
                                "mix": mix, "levels": results, "summary": summary}, indent=1))
    print("syscalls per request, least spread first:", summary["spread"])
    print(f"apps.json {'mix.scaler' if mix else 'scaler'}:", json.dumps({k: summary[k] for k in ("syscall", "calls_per_request", "cpu_per_request_ms")}))
    print(f"one pod at {TARGET:.0%} of {limit:g} core holds about {capacity:.0f} RPS; details in {path}")
