"""Open-loop load: requests leave at the profile's rate whether or not earlier ones have answered."""

import csv
import math
import statistics
import time
import urllib.error
import urllib.request
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path

from .profiles import Profile, Rate

COLUMNS = ["timestamp_epoch", "elapsed_s", "target_rps", "sent", "success", "failed", "achieved_rps", "mean_latency_ms", "p95_latency_ms"]
WORKERS = 256


def request(url: str, timeout_s: float) -> tuple[bool, float]:
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(url, timeout=timeout_s) as response:
            response.read(1)
            ok = 200 <= response.status < 400
    except (urllib.error.URLError, TimeoutError, OSError):
        ok = False
    return ok, (time.perf_counter() - started) * 1000


def p95(values: list[float]) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, max(0, math.ceil(0.95 * len(ordered)) - 1))]


def summary(results: list[tuple[bool, float]], sent: int, interval_s: float) -> list:
    latencies = [ms for _, ms in results]
    success = sum(1 for ok, _ in results if ok)
    return [sent, success, len(results) - success, round(sent / interval_s, 3),
            round(statistics.mean(latencies), 3) if latencies else 0.0, round(p95(latencies), 3)]


def run(url: str, profile: Profile, output: Path | None, timeout_s: float = 5.0, interval_s: float = 1.0) -> None:
    """Sends the profile's load at url; each row covers one interval of completed requests."""
    rate = Rate(profile)
    start = time.monotonic()
    next_send, next_row = start, start + interval_s
    pending: list[Future] = []
    sent = 0
    out = output.open("w", newline="") if output else None
    writer = csv.writer(out) if out else None
    if writer:
        writer.writerow(COLUMNS)
    with ThreadPoolExecutor(max_workers=WORKERS) as pool:
        while (now := time.monotonic()) - start < profile.duration_s:
            target = rate.at(now - start)
            if target <= 0:
                time.sleep(0.05)
            elif now >= next_send:
                pending.append(pool.submit(request, url, timeout_s))
                sent += 1
                # Late sends do not burst to catch up; the schedule restarts from now.
                next_send = max(next_send + 1 / target, now)
            else:
                time.sleep(min(0.005, next_send - now))
            if time.monotonic() >= next_row:
                done = [f for f in pending if f.done()]
                pending = [f for f in pending if not f.done()]
                elapsed = time.monotonic() - start
                if writer:
                    writer.writerow([f"{time.time():.6f}", f"{elapsed:.3f}", f"{rate.at(elapsed):.3f}",
                                     *summary([f.result() for f in done], sent, interval_s)])
                    out.flush()
                sent = 0
                next_row += interval_s
        results = [f.result() for f in pending]
    if writer and (results or sent):
        elapsed = time.monotonic() - start
        writer.writerow([f"{time.time():.6f}", f"{elapsed:.3f}", f"{rate.at(min(elapsed, profile.duration_s)):.3f}",
                         *summary(results, sent, interval_s)])
    if out:
        out.close()
