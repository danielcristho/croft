"""Burst runs scored per arm, the HPA alone against the HPA with croft scale: reaction, latency objective, failures, cost."""

import csv
import json
import re
import statistics
from collections.abc import Iterable
from pathlib import Path

from . import kube, prometheus, records
from .apps import ROOT

# Consts
SLO_P95_MS = 500
RUNS = records.DATA / "web" / "runs.jsonl"
OUTPUT = records.DATA / "web" / "compare.csv"
ARM = ("app", "profile", "scale", "probes", "scaler", "concurrent")
METRICS = [
    "hpa_first_up_s", "croft_first_up_s", "first_ready_s", "peak_replicas", "time_to_peak_s",
    "scale_down_events", "pods_created", "slo_violation_s", "failed_fraction", "success_ratio", "replica_seconds",
]
CROFT_SCALE = re.compile(r"scale (up|down) from=\d+ to=(\d+)")
HPA_NEW_SIZE = re.compile(r"New size: (\d+); reason: .*above target")
NEW_SIZE = re.compile(r"New size: (\d+);")
# croft colours its log unless NO_COLOR is set, and the scaler pod does not set it.
ANSI = re.compile(r"\x1b\[[0-9;]*m")


# burst_window is the burst's start and end in epoch ms, from the run's start and its profile.
def burst_window(record: dict) -> tuple[int, int]:
    profile = record["profile"]
    start_ms = record["started_ms"] + int(profile["burst_start_s"] * 1000)
    return start_ms, start_ms + int(profile["burst_duration_s"] * 1000)


# parse_scaler_log reads croft's decisions in the given directions as (epoch ms, new replicas) from kubectl's --timestamps prefix.
def parse_scaler_log(text: str, directions: tuple[str, ...] = ("up",)) -> list[tuple[int, int]]:
    decisions = []
    for line in text.splitlines():
        stamp, _, message = line.partition(" ")
        match = CROFT_SCALE.search(ANSI.sub("", message))
        if match and match.group(1) in directions:
            decisions.append((kube.epoch_ms(stamp), int(match.group(2))))
    return decisions


# parse_rescales reads the HPA's scale-ups as (epoch ms, new size); an aggregated event counts from its first time.
def parse_rescales(rescales: list[dict]) -> list[tuple[int, int]]:
    return [
        (rescale["first_ms"], int(match.group(1)))
        for rescale in rescales
        if (match := HPA_NEW_SIZE.search(rescale["message"]))
    ]


def seconds_after(start_ms: int, times_ms: Iterable[int]) -> float | None:
    later = [t for t in times_ms if t >= start_ms]
    return round((min(later) - start_ms) / 1000, 1) if later else None


def read_csv(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    with path.open() as rows:
        return list(csv.DictReader(rows))


# load_metrics scores the burst window of workload.csv: one row per second of completed requests.
def load_metrics(rows: list[dict], start_ms: int, end_ms: int) -> dict:
    burst = [r for r in rows if start_ms < float(r["timestamp_epoch"]) * 1000 <= end_ms]
    if not burst:
        return {}
    success, failed = sum(int(r["success"]) for r in burst), sum(int(r["failed"]) for r in burst)
    target = sum(float(r["target_rps"]) for r in burst)
    return {
        "slo_violation_s": sum(1 for r in burst if float(r["p95_latency_ms"]) > SLO_P95_MS),
        "failed_fraction": round(failed / (success + failed), 3) if success + failed else None,
        "success_ratio": round(success / target, 3) if target else None,
    }


# replica_metrics reads the cost from the Deployment's replicas in prometheus.csv, sampled every prometheus.STEP_S.
def replica_metrics(rows: list[dict]) -> dict:
    replicas = [float(r["replicas"]) for r in rows if r.get("replicas")]
    return {"replica_seconds": sum(replicas) * prometheus.STEP_S} if replicas else {}


# replica_steps lists every replica count asked of the Deployment as (epoch ms, replicas), from the deploy's size on.
# kube-state-metrics is scraped every 30 s and misses a count the HPA undoes seconds after croft asked for it.
# Kubernetes merges an event's repeats into one with a count; the HPA acts once per 15 s sync, so they are spread evenly.
def replica_steps(record: dict, rescales: list[dict], croft: list[tuple[int, int]]) -> list[tuple[int, int]]:
    asked = list(croft)
    for rescale in rescales:
        if match := NEW_SIZE.search(rescale["message"]):
            first, last, count = rescale["first_ms"], rescale.get("last_ms", rescale["first_ms"]), rescale.get("count", 1)
            asked += [(first + (last - first) * i // max(count - 1, 1), int(match.group(1))) for i in range(count)]
    start = record["started_ms"]
    return [(start, record.get("replicas") or 1), *sorted((t, n) for t, n in asked if t >= start)]


# decision_metrics scores the replica steps: the peak in the burst, when it was first asked, and the pods added over the run.
def decision_metrics(steps: list[tuple[int, int]], start_ms: int, end_ms: int) -> dict:
    at_start = [n for t, n in steps if t <= start_ms][-1]
    burst = [(t, n) for t, n in steps if start_ms < t <= end_ms]
    peak = max([at_start, *(n for _, n in burst)])
    return {
        "peak_replicas": peak,
        "time_to_peak_s": 0.0 if at_start == peak else seconds_after(start_ms, (t for t, n in burst if n == peak)),
        "pods_created": sum(max(0, b - a) for (_, a), (_, b) in zip(steps, steps[1:])),
    }


def run_metrics(record: dict, directory: Path) -> dict:
    start_ms, end_ms = burst_window(record)
    timeline_path = directory / "timeline.json"
    timeline = json.loads(timeline_path.read_text()) if timeline_path.is_file() else {}
    log_path = directory / "scaler.log"
    log = log_path.read_text() if log_path.is_file() else ""
    croft = parse_scaler_log(log)
    scale_downs = [
        event for event in timeline.get("replica_sets", [])
        if event["message"].startswith("Scaled down") and start_ms <= event["last_ms"] and event["first_ms"] <= end_ms
    ]
    return {
        "run_id": record["run_id"],
        **arm(record),
        "hpa_first_up_s": seconds_after(start_ms, (t for t, _ in parse_rescales(timeline.get("rescales", [])))),
        "croft_first_up_s": seconds_after(start_ms, (t for t, _ in croft)),
        "first_ready_s": seconds_after(start_ms, (
            pod["ready_ms"] for pod in timeline.get("pods", []) if pod["created_ms"] >= start_ms and pod["ready_ms"]
        )),
        "scale_down_events": len(scale_downs),
        **load_metrics(read_csv(directory / "workload.csv"), start_ms, end_ms),
        **replica_metrics(read_csv(directory / "prometheus.csv")),
        **decision_metrics(replica_steps(record, timeline.get("rescales", []), parse_scaler_log(log, ("up", "down"))), start_ms, end_ms),
    }


# arm names the conditions a run shares with its repetitions; older records lack app, probes, scaler and concurrent.
def arm(record: dict) -> dict:
    return {
        "app": record.get("app", "webapp"),
        "profile": record["profile"]["name"],
        "scale": record["scale"],
        "probes": record.get("probes", "default"),
        "scaler": record.get("scaler") or "hpa",
        "concurrent": record.get("concurrent", False),
    }


def mean_sd(values: list[float]) -> str:
    if not values:
        return "-"
    sd = statistics.stdev(values) if len(values) > 1 else 0.0
    return f"{statistics.mean(values):.2f} ± {sd:.2f}"


def summarize(runs: list[dict]) -> list[dict]:
    """One row per arm: run count and mean ± sd of every metric, with [k] when only k runs have it."""
    arms: dict[tuple, list[dict]] = {}
    for run in runs:
        arms.setdefault(tuple(run[key] for key in ARM), []).append(run)
    summary = []
    for key, members in arms.items():
        row = {**dict(zip(ARM, key)), "n": len(members)}
        for metric in METRICS:
            values = [run[metric] for run in members if run.get(metric) is not None]
            row[metric] = mean_sd(values) + (f" [{len(values)}]" if 0 < len(values) < len(members) else "")
        summary.append(row)
    return summary


# burst_records keeps the last record of each burst run: a rerun under the same id overwrote its directory.
def burst_records(path: Path, prefixes: list[str]) -> list[dict]:
    if not path.is_file():
        raise SystemExit(f"{path} does not exist; run a burst series first")
    latest = {}
    for line in path.read_text().splitlines():
        record = json.loads(line)
        if record.get("load") == "open-loop" and record["profile"]["name"] == "burst":
            if not prefixes or any(record["run_id"].startswith(prefix) for prefix in prefixes):
                latest[record["run_id"]] = record
    if not latest:
        raise SystemExit(f"{path} holds no burst runs{' for ' + ', '.join(prefixes) if prefixes else ''}")
    return list(latest.values())


# merge replaces the rows of runs scored again and keeps every other run, so scoring one series keeps the others.
def merge(previous: list[dict], scored: list[dict]) -> list[dict]:
    fresh = {row["run_id"]: row for row in scored}
    kept = [fresh.pop(row["run_id"], row) for row in previous]
    return [*kept, *fresh.values()]


def run(prefixes: list[str], runs_path: Path = RUNS, output: Path = OUTPUT) -> None:
    scored = [run_metrics(record, ROOT / record["directory"]) for record in burst_records(runs_path, prefixes)]
    with output.open("w", newline="") as out:
        writer = csv.DictWriter(out, fieldnames=["run_id", *ARM, *METRICS], restval="", extrasaction="ignore")
        writer.writeheader()
        writer.writerows(merge(read_csv(output), scored))
    for row in summarize(scored):
        print(f"{row['app']} {row['profile']} x{row['scale']:g}, probes {row['probes']}, {row['scaler']}{', concurrent' if row['concurrent'] else ''} (n={row['n']})")
        for metric in METRICS:
            print(f"  {metric:<18} {row[metric]}")
    print(f"per-run scores in {output}")
