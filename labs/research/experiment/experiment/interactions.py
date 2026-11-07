"""Checks of the service-interaction study (RUNNING.md section 18), read from data/web only.

  python3 -m experiment.interactions smoke <run-directory>   the call graph of a --mesh smoke run
  python3 -m experiment.interactions capacity                one meshed pod's capacity from <app>-mesh-cap-ramp
  python3 -m experiment.interactions summary [series]        acceptance columns of a series; default si
"""

import csv
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

from experiment.apps import APPS
from experiment.profiles import PROFILES
from experiment.records import DATA

WEB = DATA / "web"
BIN_RPS = 10


def rows(path: Path) -> list[dict]:
    if not path.exists():
        return []
    with path.open(newline="") as f:
        return list(csv.DictReader(f))


def p95_by_rate(workload: list[dict]) -> list[tuple[int, float]]:
    """Median p95 per 10-RPS bin of the target rate; seconds without an answer are left out."""
    bins = defaultdict(list)
    for row in workload:
        if float(row["p95_latency_ms"]) > 0:
            bins[int(float(row["target_rps"]) // BIN_RPS) * BIN_RPS].append(float(row["p95_latency_ms"]))
    return [(rate, statistics.median(values)) for rate, values in sorted(bins.items())]


def knee(table: list[tuple[int, float]]) -> int | None:
    """The first rate whose p95 stays above twice the floor, the median of the first three bins, for two bins."""
    if len(table) < 5:
        return None
    floor = statistics.median(p95 for _, p95 in table[:3])
    return next((table[i][0] for i in range(len(table) - 1) if table[i][1] > 2 * floor and table[i + 1][1] > 2 * floor), None)


def smoke(directory: Path) -> int:
    pairs = sorted({(row["src"], row["dst"]) for row in rows(directory / "edges.csv")})
    services = {row["service"] for row in rows(directory / "services.csv")}
    signals = {(row["service"], row["signal"]) for row in rows(directory / "signals.csv")}
    print(f"{directory.name}: {len(pairs)} edges, {len(services)} services")
    for src, dst in pairs:
        print(f"  {src} -> {dst}")
    problems = [p for p, bad in [("no ingress -> frontend edge in edges.csv", ("ingress", "frontend") not in pairs),
                                 ("services.csv is missing or empty", not services),
                                 ("no frontend syscalls in signals.csv", ("frontend", "syscall_rate") not in signals)] if bad]
    for problem in problems:
        print(f"  PROBLEM: {problem}")
    return 1 if problems else 0


def capacity() -> int:
    peak = PROFILES["burst"].burst_rps
    for app in sorted(APPS):
        table = p95_by_rate(rows(WEB / f"{app}-mesh-cap-ramp" / "workload.csv"))
        if not table:
            print(f"\n{app}: no {app}-mesh-cap-ramp run")
            continue
        found = knee(table)
        print(f"\n{app}\n  target_rps  median_p95_ms")
        for rate, p95 in table:
            print(f"  {rate:>10}  {p95:>13.0f}{'  <- knee' if rate == found else ''}")
        if found is None:
            print("  no knee: the ramp did not saturate one pod")
        else:
            print(f"  one pod holds ~{found} RPS; lo {2 * found / peak:.2f}, hi {4 * found / peak:.2f} (burst peak {2 * found} / {4 * found} RPS)")
    print("\nSuggestions only: choose lo and hi, and record them with the reason in RUNNING.md section 18.")
    return 0


def summary(series: str = "si") -> int:
    runs = {}
    for line in (WEB / "runs.jsonl").read_text().splitlines() if (WEB / "runs.jsonl").exists() else []:
        record = json.loads(line)
        if record["run_id"].startswith(f"{series}-"):
            runs[record["run_id"]] = record
    print(f"{'run_id':<40} {'mesh':<8} {'concurrent':<10} {'edges':>5} {'restarts':>8} {'ingress_p95_ms':>14} {'failed_pct':>10}")
    for run_id, record in sorted(runs.items()):
        directory = WEB / run_id
        edges = rows(directory / "edges.csv")
        restarts = defaultdict(list)
        for row in rows(directory / "services.csv"):
            if row["restarts"]:
                restarts[row["service"]].append(float(row["restarts"]))
        added = max((max(v) - min(v) for v in restarts.values()), default=0)
        p95 = [float(e["p95_ms"]) for e in edges if e["src"] == "ingress" and e["p95_ms"]]
        workload = rows(directory / "workload.csv")
        sent = sum(float(row["sent"]) for row in workload)
        failed = 100 * sum(float(row["failed"]) for row in workload) / sent if sent else float("nan")
        print(f"{run_id:<40} {str(record.get('mesh')):<8} {str(record.get('concurrent', False)):<10} {len({(e['src'], e['dst']) for e in edges}):>5} "
              f"{added:>8.0f} {statistics.median(p95) if p95 else float('nan'):>14.0f} {failed:>10.2f}")
    print(f"\n{len(runs)} {series}-* runs; restarts is the largest restart increase of one service in the run; failed_pct is of the requests sent")
    return 0


if __name__ == "__main__":
    match sys.argv[1:]:
        case ["smoke", directory]:
            sys.exit(smoke(Path(directory)))
        case ["capacity"]:
            sys.exit(capacity())
        case ["summary"]:
            sys.exit(summary())
        case ["summary", series]:
            sys.exit(summary(series))
        case _:
            sys.exit(__doc__)
