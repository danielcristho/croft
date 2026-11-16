"""The load map without a cluster: phase windows, the service and edge tables, the ranking and the summary."""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from experiment import loadmap

RECORD = {"run_id": "lm-x-1-burst", "app": "hotel-reservation", "scale": 1.33, "started_ms": 1_000_000,
          "profile": {"name": "burst", "burst_start_s": 180, "burst_duration_s": 120}}


def service(unix_s: int, name: str, cpu: float, throttled: float = 0.0, restarts: float = 0.0) -> dict:
    return {"unix_s": str(unix_s), "service": name, "cpu_cores": str(cpu), "cpu_throttled_fraction": str(throttled),
            "memory_bytes": "", "restarts": str(restarts), "replicas": "1", "ready_replicas": "1"}


def signal(unix_s: int, name: str, key: str, value: float) -> dict:
    return {"unix_s": str(unix_s), "service": name, "signal": "syscall_rate", "key": key, "value": str(value)}


class Phases(unittest.TestCase):
    def test_base_is_the_minute_before_the_burst(self) -> None:
        window = loadmap.phases(RECORD)
        self.assertEqual(window.burst, (1180, 1300))
        self.assertEqual(window.base, (1120, 1180))


class Tables(unittest.TestCase):
    window = loadmap.Phases((100, 160), (160, 280))

    def test_a_throttled_backend_ranks_above_an_idle_frontend(self) -> None:
        rows = [service(130, "frontend", 0.2), service(200, "frontend", 0.3),
                service(130, "rate", 0.1), service(200, "rate", 1.0, throttled=0.9, restarts=0),
                service(250, "rate", 0.9, restarts=2),
                service(200, "consul", 2.0, throttled=1.0)]
        table = loadmap.service_table("hotel-reservation", rows, [], self.window)
        ranked = loadmap.rank(table)
        self.assertEqual([r.service for r in ranked], ["rate", "frontend"])
        rate = ranked[0]
        self.assertEqual((rate.role, rate.language, rate.peak_throttled, rate.restarts), ("business", "go", 0.9, 2.0))

    def test_support_components_stay_in_the_table_but_not_the_rank(self) -> None:
        table = loadmap.service_table("hotel-reservation", [service(200, "consul", 2.0, throttled=1.0)], [], self.window)
        self.assertEqual([r.service for r in table], ["consul"])
        self.assertEqual(loadmap.rank(table), [])

    def test_syscall_rise_sums_syscalls_per_stamp(self) -> None:
        signals = [signal(130, "rate", "read", 10), signal(130, "rate", "write", 10),
                   signal(200, "rate", "read", 50), signal(200, "rate", "write", 30)]
        table = loadmap.service_table("hotel-reservation", [service(200, "rate", 1.0)], signals, self.window)
        self.assertAlmostEqual(table[0].syscall_rise, 4.0)

    def test_no_signals_leaves_the_syscall_rise_empty(self) -> None:
        table = loadmap.service_table("hotel-reservation", [service(200, "rate", 1.0)], [], self.window)
        self.assertIsNone(table[0].syscall_rise)

    def test_an_unlisted_service_gets_empty_labels(self) -> None:
        table = loadmap.service_table("hotel-reservation", [service(200, "debug", 1.0)], [], self.window)
        self.assertEqual((table[0].role, table[0].language), ("", ""))

    def test_edges_compare_base_and_burst(self) -> None:
        edges = [{"unix_s": "130", "src": "frontend", "dst": "rate", "rps": "5", "success_rate": "1.0", "p50_ms": "1", "p95_ms": "4"},
                 {"unix_s": "200", "src": "frontend", "dst": "rate", "rps": "50", "success_rate": "0.8", "p50_ms": "9", "p95_ms": "40"},
                 {"unix_s": "210", "src": "frontend", "dst": "rate", "rps": "60", "success_rate": "", "p50_ms": "", "p95_ms": ""}]
        (edge,) = loadmap.edge_table(edges, self.window)
        self.assertEqual((edge.base_rps, edge.burst_rps, edge.base_p95_ms, edge.burst_p95_ms, edge.burst_min_success),
                         (5.0, 55.0, 4.0, 40.0, 0.8))


class Summary(unittest.TestCase):
    def test_summary_prints_the_burst_runs_of_a_prefix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            web = Path(tmp)
            ramp = dict(RECORD, run_id="lm-x-1-ramp", profile={"name": "ramp"})
            (web / "runs.jsonl").write_text("\n".join(json.dumps(r) for r in (RECORD, ramp)) + "\n")
            run = web / RECORD["run_id"]
            run.mkdir()
            (run / "services.csv").write_text(
                "unix_s,service,cpu_cores,cpu_throttled_fraction,memory_bytes,restarts,replicas,ready_replicas\n"
                "1200,rate,1.0,0.9,,0,1,1\n")
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(loadmap.summary("lm-", web), 0)
        text = out.getvalue()
        self.assertIn("1 burst runs (lm-x-1-burst)", text)
        self.assertIn("rate", text)

    def test_no_runs_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
            self.assertEqual(loadmap.summary("lm-", Path(tmp)), 1)


if __name__ == "__main__":
    unittest.main()
