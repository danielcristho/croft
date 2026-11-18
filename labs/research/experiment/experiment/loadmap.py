"""Load map (#21): which services take a burst, read from a run's services.csv, signals.csv and edges.csv.

  python3 -m experiment.loadmap summary <prefix>   services and edges of the burst runs whose id starts with <prefix>

The ranking order (throttling, then peak CPU, then syscall rise) is a heuristic chosen before seeing the load-map runs.
"""

import json
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

from experiment.apps import APPS
from experiment.compare import burst_window, read_csv
from experiment.records import DATA

# Consts
WEB = DATA / "web"
BASE_S = 60


@dataclass(frozen=True)
class Phases:
    """Epoch-second windows of one run: the base load before the burst, and the burst."""

    base: tuple[int, int]
    burst: tuple[int, int]


@dataclass(frozen=True)
class ServiceRow:
    service: str
    role: str
    language: str
    peak_cpu_cores: float
    base_cpu_cores: float
    peak_throttled: float
    restarts: float
    # Burst mean over base mean of the service's summed syscall rate; None without signals.csv or a base of 0.
    syscall_rise: float | None


@dataclass(frozen=True)
class EdgeRow:
    src: str
    dst: str
    base_rps: float
    burst_rps: float
    base_p95_ms: float | None
    burst_p95_ms: float | None
    burst_min_success: float | None


def phases(record: dict) -> Phases:
    start_ms, end_ms = burst_window(record)
    start_s, end_s = start_ms // 1000, end_ms // 1000
    return Phases((start_s - BASE_S, start_s), (start_s, end_s))


def inside(row: dict, window: tuple[int, int]) -> bool:
    return window[0] <= float(row["unix_s"]) < window[1]


def numbers(rows: list[dict], column: str) -> list[float]:
    return [float(row[column]) for row in rows if row.get(column) not in (None, "")]


def mean(values: list[float]) -> float | None:
    return statistics.fmean(values) if values else None


def syscall_rate(signals: list[dict], window: tuple[int, int]) -> dict[str, float]:
    """Each service's mean summed syscall rate over the window."""
    per_stamp: dict[str, dict[float, float]] = defaultdict(lambda: defaultdict(float))
    for row in signals:
        if row["signal"] == "syscall_rate" and inside(row, window):
            per_stamp[row["service"]][float(row["unix_s"])] += float(row["value"])
    return {service: statistics.fmean(stamps.values()) for service, stamps in per_stamp.items()}


def service_table(app: str, services: list[dict], signals: list[dict], window: Phases) -> list[ServiceRow]:
    known = APPS[app].services if app in APPS else {}
    base_rate, burst_rate = syscall_rate(signals, window.base), syscall_rate(signals, window.burst)
    by_service: dict[str, list[dict]] = defaultdict(list)
    for row in services:
        by_service[row["service"]].append(row)
    table = []
    for service, rows in sorted(by_service.items()):
        base = [r for r in rows if inside(r, window.base)]
        burst = [r for r in rows if inside(r, window.burst)]
        restarts = numbers(rows, "restarts")
        rise = None
        if base_rate.get(service) and service in burst_rate:
            rise = burst_rate[service] / base_rate[service]
        label = known.get(service)
        table.append(ServiceRow(
            service=service,
            role=label.role if label else "",
            language=label.language if label else "",
            peak_cpu_cores=max(numbers(burst, "cpu_cores"), default=0.0),
            base_cpu_cores=mean(numbers(base, "cpu_cores")) or 0.0,
            peak_throttled=max(numbers(burst, "cpu_throttled_fraction"), default=0.0),
            restarts=max(restarts) - min(restarts) if restarts else 0.0,
            syscall_rise=rise,
        ))
    return table


def rank(table: list[ServiceRow]) -> list[ServiceRow]:
    """Services that serve requests, most strained first; support components are left out."""
    serving = [row for row in table if row.role != "support"]
    return sorted(serving, key=lambda r: (-r.peak_throttled, -r.peak_cpu_cores, -(r.syscall_rise or 0.0), r.service))


def edge_table(edges: list[dict], window: Phases) -> list[EdgeRow]:
    by_edge: dict[tuple[str, str], list[dict]] = defaultdict(list)
    for row in edges:
        by_edge[(row["src"], row["dst"])].append(row)
    table = []
    for (src, dst), rows in sorted(by_edge.items()):
        base = [r for r in rows if inside(r, window.base)]
        burst = [r for r in rows if inside(r, window.burst)]
        success = numbers(burst, "success_rate")
        table.append(EdgeRow(src, dst, mean(numbers(base, "rps")) or 0.0, mean(numbers(burst, "rps")) or 0.0,
                             median(numbers(base, "p95_ms")), median(numbers(burst, "p95_ms")),
                             min(success) if success else None))
    return table


def median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def burst_records(prefix: str, runs: Path) -> list[dict]:
    found = []
    for line in runs.read_text().splitlines() if runs.exists() else []:
        record = json.loads(line)
        if record["run_id"].startswith(prefix) and record.get("profile", {}).get("name") == "burst":
            found.append(record)
    return found


def mean_sd(values: list[float]) -> str:
    if not values:
        return "-"
    if len(values) == 1:
        return f"{values[0]:.2f}"
    return f"{statistics.fmean(values):.2f}±{statistics.stdev(values):.2f}"


def summary(prefix: str, web: Path = WEB) -> int:
    groups: dict[tuple[str, float], list[dict]] = defaultdict(list)
    for record in burst_records(prefix, web / "runs.jsonl"):
        groups[(record["app"], record.get("scale", 1.0))].append(record)
    if not groups:
        print(f"no burst runs start with {prefix!r} in {web / 'runs.jsonl'}")
        return 1
    for (app, scale), records in sorted(groups.items()):
        services: dict[str, list[ServiceRow]] = defaultdict(list)
        edges: dict[tuple[str, str], list[EdgeRow]] = defaultdict(list)
        for record in records:
            directory = web / record["run_id"]
            window = phases(record)
            for row in rank(service_table(app, read_csv(directory / "services.csv"), read_csv(directory / "signals.csv"), window)):
                services[row.service].append(row)
            for edge in edge_table(read_csv(directory / "edges.csv"), window):
                edges[(edge.src, edge.dst)].append(edge)
        print(f"\n{app} scale {scale}: {len(records)} burst runs ({', '.join(r['run_id'] for r in records)})")
        print(f"  {'service':<24} {'role':<9} {'language':<8} {'peak_throttled':>14} {'peak_cpu':>11} {'base_cpu':>11} {'syscall_rise':>12} {'restarts':>8}")
        order = sorted(services, key=lambda s: (-statistics.fmean(r.peak_throttled for r in services[s]),
                                                -statistics.fmean(r.peak_cpu_cores for r in services[s]), s))
        for service in order:
            rows = services[service]
            print(f"  {service:<24} {rows[0].role:<9} {rows[0].language:<8} {mean_sd([r.peak_throttled for r in rows]):>14} "
                  f"{mean_sd([r.peak_cpu_cores for r in rows]):>11} {mean_sd([r.base_cpu_cores for r in rows]):>11} "
                  f"{mean_sd([r.syscall_rise for r in rows if r.syscall_rise is not None]):>12} {mean_sd([r.restarts for r in rows]):>8}")
        support = {name for name, label in (APPS[app].services if app in APPS else {}).items() if label.role == "support"}
        # Calls to support components, such as consul's long polls, say nothing about the burst.
        edges = {key: rows for key, rows in edges.items() if not support & set(key)}
        if edges:
            print(f"  {'edge':<40} {'base_rps':>9} {'burst_rps':>10} {'base_p95':>9} {'burst_p95':>10} {'min_success':>11}")
            busy = sorted(edges, key=lambda e: -statistics.fmean(x.burst_rps for x in edges[e]))
            for key in busy:
                rows = [x for x in edges[key] if x.burst_rps > 0]
                if not rows:
                    continue
                print(f"  {key[0] + ' -> ' + key[1]:<40} {mean_sd([x.base_rps for x in rows]):>9} {mean_sd([x.burst_rps for x in rows]):>10} "
                      f"{mean_sd([x.base_p95_ms for x in rows if x.base_p95_ms is not None]):>9} "
                      f"{mean_sd([x.burst_p95_ms for x in rows if x.burst_p95_ms is not None]):>10} "
                      f"{mean_sd([x.burst_min_success for x in rows if x.burst_min_success is not None]):>11}")
    print(f"\nmean±sd over runs; base is the {BASE_S} s before the burst; support components are left out; "
          "the order is a heuristic: peak throttled share, then peak CPU cores")
    return 0


if __name__ == "__main__":
    match sys.argv[1:]:
        case ["summary", prefix]:
            sys.exit(summary(prefix))
        case _:
            sys.exit(__doc__)
