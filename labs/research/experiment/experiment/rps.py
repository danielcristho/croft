"""Open-loop load: requests leave at the profile's rate whether or not earlier ones have answered."""

import asyncio
import contextlib
import csv
import itertools
import math
import random
import socket
import statistics
import time
from collections import Counter
from pathlib import Path
from urllib.parse import urlsplit

from .profiles import Endpoint, Profile, Rate

COLUMNS = ["timestamp_epoch", "elapsed_s", "target_rps", "sent", "success", "failed", "achieved_rps", "mean_latency_ms", "p95_latency_ms", "in_flight"]
ENDPOINT_COLUMNS = ["timestamp_epoch", "elapsed_s", "endpoint", "sent", "success", "failed", "achieved_rps", "mean_latency_ms", "p95_latency_ms"]
# 2: latency from the scheduled send, a deadline per request, no worker limit, the host resolved once.
GENERATOR = 2


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


def resolve(host: str) -> tuple[str, int]:
    """The address, resolved once so a slow resolver cannot stall the load."""
    parts = urlsplit(host)
    if parts.scheme != "http":
        raise ValueError(f"{host}: only http is supported")
    return socket.getaddrinfo(parts.hostname, parts.port or 80, type=socket.SOCK_STREAM)[0][4][:2]


def head(host: str, endpoint: Endpoint, n: int = 0, user: str = "") -> bytes:
    """The request bytes for one endpoint, body included, with {n} replaced by the send's number and {user} by its user."""
    def fill(text: str) -> str:
        return text.replace("{n}", str(n)).replace("{user}", user)

    body = fill(endpoint.body).encode()
    lines = [f"{endpoint.method} {fill(endpoint.path)} HTTP/1.1", f"Host: {urlsplit(host).netloc}", "Connection: close",
             *(f"{name}: {fill(value)}" for name, value in endpoint.headers.items())]
    if endpoint.method == "POST":
        lines += [f"Content-Type: {endpoint.content_type or 'application/x-www-form-urlencoded'}", f"Content-Length: {len(body)}"]
    return ("\r\n".join(lines) + "\r\n\r\n").encode() + body


async def exchange(address: tuple[str, int], head: bytes) -> bool:
    """Whether the answer is 2xx or 3xx; the request ends at the first byte of the body."""
    reader, writer = await asyncio.open_connection(*address)
    try:
        writer.write(head)
        await writer.drain()
        headers = await reader.readuntil(b"\r\n\r\n")
        await reader.read(1)
        return 200 <= int(headers.split(maxsplit=2)[1]) < 400
    finally:
        writer.close()


async def chain(address: tuple[str, int], heads: list[bytes]) -> bool:
    for head in heads:
        if not await exchange(address, head):
            return False
    return True


async def request(address: tuple[str, int], heads: list[bytes], scheduled: float, timeout_s: float) -> tuple[bool, float]:
    """One request, or a chain sent in order, timed from when it should have left, so a late send counts as latency."""
    try:
        ok = await asyncio.wait_for(chain(address, heads), timeout_s)
    except (OSError, TimeoutError, ValueError, IndexError, asyncio.IncompleteReadError, asyncio.LimitOverrunError):
        ok = False
    return ok, (time.monotonic() - scheduled) * 1000


class Rows:
    """The CSV rows of one run: workload.csv per interval, and endpoints.csv per interval and endpoint when asked."""

    def __init__(self, workload, endpoints, interval_s: float):
        self.workload, self.endpoints, self.interval_s = workload, endpoints, interval_s

    def write(self, elapsed: float, target: float, done: list[tuple[str, bool, float]], sent: Counter, in_flight: int) -> None:
        stamp = f"{time.time():.6f}"
        if self.workload:
            self.workload.writerow([stamp, f"{elapsed:.3f}", f"{target:.3f}",
                                    *summary([(ok, ms) for _, ok, ms in done], sum(sent.values()), self.interval_s), in_flight])
        if self.endpoints:
            for name in sorted(set(sent) | {name for name, _, _ in done}):
                mine = [(ok, ms) for n, ok, ms in done if n == name]
                self.endpoints.writerow([stamp, f"{elapsed:.3f}", name, *summary(mine, sent[name], self.interval_s)])


async def offer(host: str, endpoints: list[Endpoint], profile: Profile, rows: Rows, timeout_s: float, interval_s: float,
                run: str, users: int) -> None:
    address = resolve(host)
    named = {endpoint.name: endpoint for endpoint in endpoints}
    weights = list(itertools.accumulate(endpoint.weight for endpoint in endpoints))
    # Seeded from the profile, so a repeated run sends the same sequence of endpoints and users; the derived seeds keep
    # both apart from Rate's.
    pick = random.Random(f"{profile.seed}:endpoints")
    people = random.Random(f"{profile.seed}:users")
    # Start from the clock, so a cart one run leaves behind is never another run's.
    numbers = itertools.count(time.time_ns() // 1000)
    rate = Rate(profile)
    start = time.monotonic()
    next_send, next_row = start, start + interval_s
    pending: dict[asyncio.Task, str] = {}
    sent: Counter = Counter()
    while (now := time.monotonic()) - start < profile.duration_s:
        rps = rate.at(now - start)
        if rps <= 0:
            await asyncio.sleep(0.05)
        elif now >= next_send:
            endpoint = endpoints[pick.choices(range(len(endpoints)), cum_weights=weights)[0]]
            steps = [named[endpoint.before], endpoint] if endpoint.before else [endpoint]
            n, user = next(numbers), f"{run}-{people.randrange(users)}"
            heads = [head(host, step, n, user) for step in steps]
            pending[asyncio.create_task(request(address, heads, next_send, timeout_s))] = endpoint.name
            sent[endpoint.name] += 1
            # Late sends do not burst to catch up; the schedule restarts from now.
            next_send = max(next_send + 1 / rps, now)
            await asyncio.sleep(0)
        else:
            await asyncio.sleep(min(0.005, next_send - now))
        if time.monotonic() >= next_row:
            done = [task for task in pending if task.done()]
            finished = [(pending.pop(task), *task.result()) for task in done]
            elapsed = time.monotonic() - start
            rows.write(elapsed, rate.at(elapsed), finished, sent, len(pending))
            sent = Counter()
            next_row += interval_s
    results = await asyncio.gather(*pending)
    finished = [(name, *result) for name, result in zip(pending.values(), results)]
    if finished or sent:
        elapsed = time.monotonic() - start
        rows.write(elapsed, rate.at(min(elapsed, profile.duration_s)), finished, sent, 0)


def run(host: str, endpoints: list[Endpoint], profile: Profile, output: Path | None, by_endpoint: Path | None = None,
        timeout_s: float = 5.0, interval_s: float = 1.0, run: str = "load", users: int = 1) -> None:
    """Sends the profile's load at the host's endpoints; each row covers one interval of completed requests."""
    with contextlib.ExitStack() as stack:
        writers = []
        for path, columns in ((output, COLUMNS), (by_endpoint, ENDPOINT_COLUMNS)):
            writer = csv.writer(stack.enter_context(path.open("w", newline="", buffering=1))) if path else None
            if writer:
                writer.writerow(columns)
            writers.append(writer)
        asyncio.run(offer(host, endpoints, profile, Rows(*writers, interval_s), timeout_s, interval_s, run, users))
