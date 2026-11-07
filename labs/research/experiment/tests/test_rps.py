"""The load generator against local servers: the deadline, the rate, late sends and failed answers."""

import asyncio
import csv
import socket
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

from experiment import rps
from experiment.profiles import Profile

OK = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
ERROR = b"HTTP/1.1 500 Oops\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno"


class Server:
    """A local HTTP server on its own loop; handler(reader, writer) answers each connection."""

    def __init__(self, handler):
        self.handler = handler
        self.loop = asyncio.new_event_loop()
        self.ready = threading.Event()
        self.open = 0
        self.peak = 0
        self.thread = threading.Thread(target=self.serve, daemon=True)

    def serve(self):
        asyncio.set_event_loop(self.loop)
        self.server = self.loop.run_until_complete(asyncio.start_server(self.answer, "127.0.0.1", 0, backlog=4096))
        self.port = self.server.sockets[0].getsockname()[1]
        self.ready.set()
        self.loop.run_forever()

    async def answer(self, reader, writer):
        self.open += 1
        self.peak = max(self.peak, self.open)
        try:
            await reader.readuntil(b"\r\n\r\n")
            await self.handler(writer)
            await writer.drain()
        except (OSError, asyncio.IncompleteReadError):
            pass
        finally:
            self.open -= 1
            writer.close()

    def __enter__(self):
        self.thread.start()
        self.ready.wait()
        return f"http://127.0.0.1:{self.port}/x?y=1"

    def __exit__(self, *_):
        async def shutdown():
            self.server.close()
            tasks = [task for task in asyncio.all_tasks() if task is not asyncio.current_task()]
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            self.loop.stop()

        self.loop.call_soon_threadsafe(self.loop.create_task, shutdown())
        self.thread.join(timeout=5)
        self.loop.close()


def load(url: str, seconds: float, rate: float, timeout_s: float = 5.0) -> list[dict]:
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "workload.csv"
        rps.run(url, Profile("constant", seconds, rps=rate), path, timeout_s=timeout_s)
        with path.open(newline="") as f:
            return list(csv.DictReader(f))


def total(rows: list[dict], column: str) -> float:
    return sum(float(row[column]) for row in rows)


class Generator(unittest.TestCase):
    def test_offers_the_rate_and_counts_answers(self):
        async def answer(writer):
            writer.write(OK)

        with Server(answer) as url:
            rows = load(url, 2, 100)
        self.assertAlmostEqual(total(rows, "sent"), 200, delta=4)
        self.assertEqual(total(rows, "success"), total(rows, "sent"))
        self.assertEqual(total(rows, "failed"), 0)

    def test_server_errors_fail(self):
        async def answer(writer):
            writer.write(ERROR)

        with Server(answer) as url:
            rows = load(url, 1, 20)
        self.assertEqual(total(rows, "success"), 0)
        self.assertEqual(total(rows, "failed"), total(rows, "sent"))

    def test_deadline_covers_an_answer_that_dribbles(self):
        async def answer(writer):
            for byte in OK:
                writer.write(bytes([byte]))
                await writer.drain()
                await asyncio.sleep(0.1)

        with Server(answer) as url:
            rows = load(url, 1, 5, timeout_s=0.5)
        self.assertEqual(total(rows, "failed"), total(rows, "sent"))
        self.assertLess(max(float(row["p95_latency_ms"]) for row in rows), 800)

    def test_in_flight_is_not_capped(self):
        async def answer(writer):
            await asyncio.sleep(2)
            writer.write(OK)

        server = Server(answer)
        with server as url:
            rows = load(url, 2, 300)
        self.assertGreater(server.peak, 256 + 100)
        self.assertGreater(max(int(row["in_flight"]) for row in rows), 256)
        self.assertEqual(total(rows, "failed"), 0)

    def test_host_resolves_once(self):
        async def answer(writer):
            writer.write(OK)

        with Server(answer) as url:
            with mock.patch.object(socket, "getaddrinfo", wraps=socket.getaddrinfo) as resolve:
                load(url, 1, 20)
        hosts = [call for call in resolve.call_args_list if call.args[0] == "127.0.0.1" and call.kwargs.get("type") == socket.SOCK_STREAM]
        self.assertEqual(len(hosts), 1)

    def test_latency_counts_a_late_send(self):
        async def run():
            scheduled = time.monotonic() - 1
            with mock.patch.object(rps, "exchange", mock.AsyncMock(return_value=True)):
                return await rps.request(("127.0.0.1", 1), b"", scheduled, 1)

        ok, ms = asyncio.run(run())
        self.assertTrue(ok)
        self.assertGreaterEqual(ms, 1000)

    def test_request_keeps_the_path_and_host(self):
        address, head = rps.target("http://127.0.0.1:8080/x?y=1")
        self.assertEqual(address, ("127.0.0.1", 8080))
        self.assertTrue(head.startswith(b"GET /x?y=1 HTTP/1.1\r\nHost: 127.0.0.1:8080\r\n"))


if __name__ == "__main__":
    unittest.main()
