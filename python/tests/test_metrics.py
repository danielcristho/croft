"""Query building and frame shape of croft.metrics, against a recorded Prometheus answer."""

import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

from croft import metrics

CONTAINER = "0af6e13c387b"


class Recorded:
    def __init__(self, streams):
        self.streams, self.calls = streams, []

    def query_range(self, query, start_s, end_s, step_s):
        self.calls.append((query, start_s, end_s, step_s))
        return self.streams


def stream(pod, syscall, values):
    labels = {"container_id": CONTAINER, "namespace": "boutique", "pod": pod, "container": "server", "syscall": syscall}
    return {"metric": labels, "values": values}


class PromqlTest(unittest.TestCase):
    def test_gauge_joins_the_deployment_pods(self):
        query = metrics.promql(metrics.SIGNALS["syscall_rate"], "boutique", "frontend", {"syscall": "sendto"}, True)
        self.assertTrue(query.startswith('croft_container_syscall_rate{syscall="sendto"} * on (container_id)'))
        self.assertIn('kube_pod_container_info{namespace="boutique",pod=~"frontend-[a-z0-9]+-[a-z0-9]+"}', query)
        self.assertIn('"[a-z-]+://(.+)"', query)

    def test_counter_is_a_rate(self):
        query = metrics.promql(metrics.SIGNALS["socket_io"], None, None, None, False)
        self.assertEqual(query, "rate(croft_container_socket_io_total[15s])")

    def test_long_steps_average_and_widen(self):
        gauge = metrics.promql(metrics.SIGNALS["cpu_cores"], None, None, None, False, 60)
        counter = metrics.promql(metrics.SIGNALS["connects"], None, None, None, False, 60)
        self.assertEqual(gauge, "avg_over_time(croft_container_cpu_cores[60s])")
        self.assertEqual(counter, "rate(croft_container_connects_total[60s])")

    def test_unknown_label_is_refused(self):
        with self.assertRaisesRegex(ValueError, "no label direction"):
            metrics.promql(metrics.SIGNALS["cpu_cores"], None, None, {"direction": "read"}, True)


class SignalTest(unittest.TestCase):
    def test_rows_carry_pod_deployment_and_label(self):
        prometheus = Recorded(
            [stream("frontend-7d9f8c6b5-x2k4p", "sendto", [[1000, "4.5"], [1005, "NaN"], [1010, "6"]])]
        )
        frame = metrics.syscall_rate("boutique", "frontend", since=900, until=1100, prometheus=prometheus)
        self.assertEqual(
            list(frame.columns), ["time", "namespace", "deployment", "pod", "container", "syscall", "value"]
        )
        self.assertEqual(frame["value"].tolist(), [4.5, 6.0])
        self.assertEqual(set(frame["deployment"]), {"frontend"})
        self.assertEqual(frame.attrs["unit"], "calls/s")

    def test_step_keeps_the_point_limit(self):
        prometheus = Recorded([])
        frame = metrics.cpu_cores(since="7d", until=10**9, prometheus=prometheus)
        _, start_s, end_s, step_s = prometheus.calls[0]
        self.assertEqual(end_s - start_s, 7 * 86400)
        self.assertEqual(step_s, 7 * 86400 / metrics.MAX_POINTS)
        self.assertTrue(frame.empty)

    def test_wide_sums_per_pod(self):
        values = [[1000, "1"], [1005, "2"]]
        prometheus = Recorded(
            [stream("frontend-7d9f8c6b5-x2k4p", "read", values), stream("frontend-7d9f8c6b5-x2k4p", "write", values)]
        )
        table = metrics.wide(metrics.syscall_rate(since=900, until=1100, prometheus=prometheus))
        self.assertEqual(table["frontend-7d9f8c6b5-x2k4p"].tolist(), [2.0, 4.0])


class TimeTest(unittest.TestCase):
    def test_durations_count_back_from_until(self):
        self.assertEqual(metrics.window("1h30m", 10_000, None)[0], 10_000 - 5400)

    def test_dates_read_as_utc(self):
        self.assertEqual(metrics.window("2026-11-18", "2026-11-18 01:00", None)[:2], (1794960000.0, 1794963600.0))

    def test_since_after_until_is_refused(self):
        with self.assertRaises(ValueError):
            metrics.window(2000, 1000, None)

    def test_deployment_of_a_statefulset_pod_is_none(self):
        self.assertIsNone(metrics.deployment_of("redis-0"))
        self.assertEqual(metrics.deployment_of("checkout-service-5c8f9d7b4-abcde"), "checkout-service")


EXPOSITION = b"""# TYPE croft_poll_unix_ms gauge
croft_poll_unix_ms 1794960000000
# TYPE croft_container_syscall_rate gauge
croft_container_syscall_rate{container_id="0af6",syscall="sendto"} 12.5
croft_container_connects_total{container_id="0af6",address="10.0.0.1",port="80"} 3
other_metric 1
"""


class Exposition(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(EXPOSITION)

    def log_message(self, *args):
        pass


class LiveTest(unittest.TestCase):
    def test_reads_the_newest_poll(self):
        server = HTTPServer(("127.0.0.1", 0), Exposition)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            frame = metrics.live(f"127.0.0.1:{server.server_port}")
        finally:
            server.shutdown()
            server.server_close()
        self.assertEqual(frame["signal"].tolist(), ["syscall_rate", "connects"])
        self.assertEqual(frame.loc[0, "syscall"], "sendto")
        self.assertEqual(frame.loc[1, "value"], 3.0)
        self.assertEqual(str(frame.attrs["poll_time"]), "2026-11-18 00:00:00+00:00")


if __name__ == "__main__":
    unittest.main()
