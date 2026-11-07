"""Open-loop load: requests leave at the profile's rate whether or not earlier ones have answered."""

import asyncio
import csv
import math
import socket
import statistics
import time
from pathlib import Path
from urllib.parse import urlsplit

from .profiles import Profile, Rate

COLUMNS = ["timestamp_epoch", "elapsed_s", "target_rps", "sent", "success", "failed", "achieved_rps", "mean_latency_ms", "p95_latency_ms", "in_flight"]
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


def target(url: str) -> tuple[tuple[str, int], bytes]:
    """The address, resolved once so a slow resolver cannot stall the load, and the request to send there."""
    parts = urlsplit(url)
    if parts.scheme != "http":
        raise ValueError(f"{url}: only http is supported")
    port = parts.port or 80
    address = socket.getaddrinfo(parts.hostname, port, type=socket.SOCK_STREAM)[0][4][:2]
    path = parts.path or "/"
    if parts.query:
        path += f"?{parts.query}"
    return address, f"GET {path} HTTP/1.1\r\nHost: {parts.netloc}\r\nConnection: close\r\n\r\n".encode()


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


async def request(address: tuple[str, int], head: bytes, scheduled: float, timeout_s: float) -> tuple[bool, float]:
    """One request, timed from when it should have left, so a late send counts as latency."""
    try:
        ok = await asyncio.wait_for(exchange(address, head), timeout_s)
    except (OSError, TimeoutError, ValueError, IndexError, asyncio.IncompleteReadError, asyncio.LimitOverrunError):
        ok = False
    return ok, (time.monotonic() - scheduled) * 1000


async def offer(url: str, profile: Profile, writer, timeout_s: float, interval_s: float) -> None:
    address, head = target(url)
    rate = Rate(profile)
    start = time.monotonic()
    next_send, next_row = start, start + interval_s
    pending: set[asyncio.Task] = set()
    sent = 0
    while (now := time.monotonic()) - start < profile.duration_s:
        rps = rate.at(now - start)
        if rps <= 0:
            await asyncio.sleep(0.05)
        elif now >= next_send:
            pending.add(asyncio.create_task(request(address, head, next_send, timeout_s)))
            sent += 1
            # Late sends do not burst to catch up; the schedule restarts from now.
            next_send = max(next_send + 1 / rps, now)
            await asyncio.sleep(0)
        else:
            await asyncio.sleep(min(0.005, next_send - now))
        if time.monotonic() >= next_row:
            done = {task for task in pending if task.done()}
            pending -= done
            elapsed = time.monotonic() - start
            if writer:
                writer.writerow([f"{time.time():.6f}", f"{elapsed:.3f}", f"{rate.at(elapsed):.3f}",
                                 *summary([task.result() for task in done], sent, interval_s), len(pending)])
            sent = 0
            next_row += interval_s
    results = await asyncio.gather(*pending)
    if writer and (results or sent):
        elapsed = time.monotonic() - start
        writer.writerow([f"{time.time():.6f}", f"{elapsed:.3f}", f"{rate.at(min(elapsed, profile.duration_s)):.3f}",
                         *summary(results, sent, interval_s), 0])


def run(url: str, profile: Profile, output: Path | None, timeout_s: float = 5.0, interval_s: float = 1.0) -> None:
    """Sends the profile's load at url; each row covers one interval of completed requests."""
    if not output:
        asyncio.run(offer(url, profile, None, timeout_s, interval_s))
        return
    with output.open("w", newline="", buffering=1) as out:
        writer = csv.writer(out)
        writer.writerow(COLUMNS)
        asyncio.run(offer(url, profile, writer, timeout_s, interval_s))
