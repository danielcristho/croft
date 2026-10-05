"""Node speed: CPU time per webapp request on each node, measured with one pinned replica under the same load."""

import csv
import json
import os
import shutil
import subprocess
from pathlib import Path

from . import kube, records, web

# Speed is relative to the fastest node, so effective CPU capacity is cores × speed.
OUTPUT = records.DATA / "node-speed.json"
DEFAULT_CROFT_DATA = Path(__file__).resolve().parents[4] / "research" / "data" / "nodes"


def run_id(prefix: str, node: str, users: int) -> str:
    return f"{prefix}-{node}-u{users}"


def measure(prefix: str, nodes: list[str], users: int, warmup: int, seconds: int, cooldown: int) -> None:
    try:
        for node in nodes:
            web.run(f"{prefix}-{node}", [users], warmup, seconds, cooldown, 1, None, 0, node=node)
    finally:
        web.pin(None)


def croft() -> str:
    binary = os.environ.get("CROFT_BIN") or shutil.which("croft")
    if not binary:
        raise SystemExit("croft not found; install it to PATH or set CROFT_BIN")
    return binary


# cpu_cores reads the container's mean CPU over the run window from croft's dataset.
def cpu_cores(container: str, croft_data: Path, start_ms: int, end_ms: int) -> float | None:
    result = subprocess.run(
        [croft(), "profile", container[:12], "--dataset", str(croft_data), "--from-ms", str(start_ms), "--to-ms", str(end_ms)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    profiles = [p for p in json.loads(result.stdout) if p["container_id"] == container and p["cpu_cores"]]
    return profiles[0]["cpu_cores"]["mean"] if profiles else None


def locust_totals(directory: Path) -> tuple[int, float]:
    with (directory / "locust_stats.csv").open() as stats:
        row = next(r for r in csv.DictReader(stats) if r["Name"] == "Aggregated")
    return int(row["Request Count"]), float(row["50%"])


def latest_runs(prefix: str, nodes: list[str], users: int) -> dict[str, dict]:
    wanted = {run_id(prefix, node, users): node for node in nodes}
    found = {}
    with (records.DATA / "web" / "runs.jsonl").open() as runs:
        for line in runs:
            record = json.loads(line)
            if record["run_id"] in wanted:
                found[wanted[record["run_id"]]] = record
    return found


def compute(prefix: str, nodes: list[str], users: int, croft_data: Path) -> dict:
    measured = {}
    for node, run in latest_runs(prefix, nodes, users).items():
        container = next((c for c, n in run["containers"].items() if n == node), None)
        cores = cpu_cores(container, croft_data, run["started_ms"], run["ended_ms"]) if container else None
        requests, p50 = locust_totals(web.ROOT / run["directory"])
        rps = requests / ((run["ended_ms"] - run["started_ms"]) / 1000)
        if cores is None or rps == 0:
            print(f"{node}: no croft rows or no requests for {run['run_id']}, skipped")
            continue
        measured[node] = {
            "cpu_ms_per_request": round(1000 * cores / rps, 2),
            "cpu_cores": round(cores, 3),
            "requests_per_s": round(rps, 2),
            "p50_ms": p50,
            "run_id": run["run_id"],
        }
    if not measured:
        raise SystemExit(f"no usable runs named {prefix}-<node>-u{users}")
    fastest = min(measured, key=lambda node: measured[node]["cpu_ms_per_request"])
    reference = measured[fastest]["cpu_ms_per_request"]
    for values in measured.values():
        values["speed"] = round(reference / values["cpu_ms_per_request"], 3)
    return {"run_prefix": prefix, "users": users, "reference_node": fastest, "nodes": measured}


def run(prefix: str, nodes: list[str] | None, users: int, warmup: int, seconds: int, cooldown: int, croft_data: Path, from_runs: bool) -> None:
    nodes = nodes or kube.ready_nodes()
    if not from_runs:
        measure(prefix, nodes, users, warmup, seconds, cooldown)
        subprocess.run([croft(), "collect", "--out", str(croft_data)], check=True)
    result = compute(prefix, nodes, users, croft_data)
    OUTPUT.write_text(json.dumps(result, indent=1) + "\n")
    for node, values in sorted(result["nodes"].items(), key=lambda item: -item[1]["speed"]):
        print(f"{node:<16} {values['cpu_ms_per_request']:>7.2f} ms/request  speed {values['speed']:.3f}")
    print(f"wrote {OUTPUT}")
