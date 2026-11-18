"""The load generator against local servers: the deadline, the rate, late sends, failed answers and the endpoint mix."""

import asyncio
import collections
import csv
import random
import socket
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

from experiment import rps
from experiment.profiles import Endpoint, Profile

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
        return f"http://127.0.0.1:{self.port}"

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


def load(host: str, seconds: float, rate: float, timeout_s: float = 5.0) -> list[dict]:
    return mixed(host, [Endpoint("default", "/x?y=1")], seconds, rate, timeout_s)[0]


def mixed(host: str, endpoints: list[Endpoint], seconds: float, rate: float, timeout_s: float = 5.0) -> tuple[list[dict], list[dict]]:
    """The rows of workload.csv and endpoints.csv."""
    with tempfile.TemporaryDirectory() as directory:
        workload, by_endpoint = Path(directory) / "workload.csv", Path(directory) / "endpoints.csv"
        rps.run(host, endpoints, Profile("constant", seconds, rps=rate), workload, by_endpoint, timeout_s=timeout_s)
        with workload.open(newline="") as w, by_endpoint.open(newline="") as e:
            return list(csv.DictReader(w)), list(csv.DictReader(e))


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
                return await rps.request(("127.0.0.1", 1), [b""], scheduled, 1)

        ok, ms = asyncio.run(run())
        self.assertTrue(ok)
        self.assertGreaterEqual(ms, 1000)

    def test_request_keeps_the_path_and_host(self):
        self.assertEqual(rps.resolve("http://127.0.0.1:8080"), ("127.0.0.1", 8080))
        head = rps.head("http://127.0.0.1:8080", Endpoint("default", "/x?y=1"))
        self.assertEqual(head, b"GET /x?y=1 HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nConnection: close\r\n\r\n")

    def test_post_carries_its_body_and_headers(self):
        endpoint = Endpoint.parse({"name": "add", "path": "/cart", "method": "POST", "body": {"id": 1}, "headers": {"Cookie": "s=1"}})
        head = rps.head("http://h:80", endpoint)
        self.assertIn(b"Cookie: s=1\r\n", head)
        self.assertIn(b"Content-Type: application/json\r\nContent-Length: 8\r\n\r\n", head)
        self.assertTrue(head.endswith(b'{"id":1}'))

    def test_n_is_filled_in_path_body_and_headers(self):
        endpoint = Endpoint.parse({"name": "a", "path": "/u/{n}", "method": "POST", "body": {"user": "u{n}"}, "headers": {"X": "{n}"}})
        head = rps.head("http://h:80", endpoint, 7)
        self.assertTrue(head.startswith(b"POST /u/7 HTTP/1.1"))
        self.assertIn(b"X: 7\r\n", head)
        self.assertTrue(head.endswith(b'{"user":"u7"}'))

    def test_user_is_filled_beside_n(self):
        endpoint = Endpoint.parse({"name": "a", "path": "/cart?s=croft-{user}", "headers": {"Cookie": "id=croft-{user}"}})
        head = rps.head("http://h:80", endpoint, 7, "run1-3")
        self.assertTrue(head.startswith(b"GET /cart?s=croft-run1-3 HTTP/1.1"))
        self.assertIn(b"Cookie: id=croft-run1-3\r\n", head)

    def test_users_belong_to_the_run_and_repeat_with_the_seed(self):
        async def answer(writer):
            writer.write(OK)

        def users(run: str) -> list[str]:
            endpoints = [Endpoint("cart", "/cart/{user}")]
            with Server(answer) as host, mock.patch.object(rps, "exchange", wraps=rps.exchange) as exchange:
                with tempfile.TemporaryDirectory() as directory:
                    rps.run(host, endpoints, Profile("constant", 1, rps=40), Path(directory) / "w.csv", run=run, users=5)
                return [call.args[1].split(b" ")[1].decode().removeprefix("/cart/") for call in exchange.call_args_list]

        first, again = users("burst-a"), users("burst-a")
        self.assertTrue(first and all(u.startswith("burst-a-") for u in first))
        self.assertLessEqual({u.rsplit("-", 1)[1] for u in first}, {str(k) for k in range(5)})
        shared = min(len(first), len(again))
        self.assertEqual(first[:shared], again[:shared])

    def test_endpoint_picks_do_not_follow_the_rate_stream(self):
        seed = 42
        rate, picks = random.Random(seed), random.Random(f"{seed}:endpoints")
        self.assertNotEqual([rate.random() for _ in range(5)], [picks.random() for _ in range(5)])

    def test_before_sends_its_endpoint_first_with_the_same_n(self):
        seen = []

        async def answer(writer):
            writer.write(OK)

        endpoints = [Endpoint("add", "/add/{n}", weight=0), Endpoint("buy", "/buy/{n}", before="add")]
        with Server(answer) as host, mock.patch.object(rps, "exchange", wraps=rps.exchange) as exchange:
            workload, by_endpoint = mixed(host, endpoints, 1, 10)
            seen = [call.args[1].split(b" ")[1].decode() for call in exchange.call_args_list]
        self.assertEqual({row["endpoint"] for row in by_endpoint}, {"buy"})
        self.assertEqual(total(workload, "failed"), 0)
        pairs = list(zip(seen[::2], seen[1::2]))
        self.assertTrue(pairs and all(a.startswith("/add/") and b == a.replace("/add/", "/buy/") for a, b in pairs))

    def test_weighted_pick_follows_the_weights(self):
        # offer draws with the same call; 10k draws land within 2 points of each share.
        pick, weights = random.Random(42), [60, 99, 99.5, 100]
        draws = collections.Counter(pick.choices(range(4), cum_weights=weights)[0] for _ in range(10_000))
        for i, share in enumerate((0.60, 0.39, 0.005, 0.005)):
            self.assertAlmostEqual(draws[i] / 10_000, share, delta=0.02)

    def test_mix_writes_each_endpoint_and_keeps_workload_columns(self):
        async def answer(writer):
            writer.write(OK)

        endpoints = [Endpoint("a", "/a", weight=3), Endpoint("b", "/b", weight=1)]
        with Server(answer) as host:
            workload, by_endpoint = mixed(host, endpoints, 2, 100)
        self.assertEqual(list(workload[0]), rps.COLUMNS)
        self.assertEqual(list(by_endpoint[0]), rps.ENDPOINT_COLUMNS)
        sent = collections.Counter()
        for row in by_endpoint:
            sent[row["endpoint"]] += int(row["sent"])
        self.assertEqual(sum(sent.values()), total(workload, "sent"))
        self.assertAlmostEqual(sent["a"] / sum(sent.values()), 0.75, delta=0.1)


if __name__ == "__main__":
    unittest.main()
