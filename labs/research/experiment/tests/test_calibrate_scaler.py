"""calibrate_scaler: picking the syscall whose calls per request hold across load levels, from answered requests."""

import csv
import tempfile
import unittest
from pathlib import Path

from experiment import calibrate_scaler


def level(rps: float, cpu: float, write: float, futex: float, close: float, sent: float | None = None) -> dict:
    return {"achieved_rps": sent or rps, "answered_rps": rps, "failed": 0,
            "metrics": {"cpu_cores": cpu, "throttled": 0.0, "write": write, "futex": futex, "close": close}}


class SummarizeTest(unittest.TestCase):
    def test_picks_the_steadiest_syscall_and_mean_cpu(self) -> None:
        levels = {
            20: level(20, 0.036, 227, 916, 0.4),
            50: level(50, 0.0825, 569, 2199, 1.0),
            100: level(100, 0.155, 1142, 4277, 2.0),
        }
        summary = calibrate_scaler.summarize(levels)
        self.assertEqual(summary["syscall"], "write")
        self.assertAlmostEqual(summary["calls_per_request"], 11.38, places=2)
        self.assertAlmostEqual(summary["cpu_per_request_ms"], 1.67, places=2)
        # close falls below one call per two requests at 20 RPS, so it is not a candidate.
        self.assertNotIn("close", summary["spread"])

    def test_per_request_figures_divide_by_answered_not_sent(self) -> None:
        # A pod that answers 20 of the 100 RPS sent: 0.118 cores is 5.9 ms per answered request, not 1.18 per sent one.
        levels = {20: level(20, 0.118, 228, 900, 1, sent=100), 10: level(10, 0.059, 114, 450, 1)}
        summary = calibrate_scaler.summarize(levels)
        self.assertAlmostEqual(summary["cpu_per_request_ms"], 5.9, places=2)
        self.assertAlmostEqual(summary["calls_per_request"], 11.4, places=2)


class LoadStatsTest(unittest.TestCase):
    def test_answered_rps_counts_successes_after_the_settle_time(self) -> None:
        columns = ["timestamp_epoch", "elapsed_s", "target_rps", "sent", "success", "failed", "achieved_rps",
                   "mean_latency_ms", "p95_latency_ms", "in_flight"]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "level.csv"
            with path.open("w", newline="") as f:
                writer = csv.writer(f)
                writer.writerow(columns)
                for second in range(30):
                    # Before the settle time every request succeeds; after it, 20 of 50 do.
                    success, failed = (50, 0) if second < calibrate_scaler.SETTLE_S else (20, 30)
                    writer.writerow([second, second, 50, 50, success, failed, 50.0, 10, 20, 0])
            stats = calibrate_scaler.load_stats(path)
        self.assertEqual((stats["achieved_rps"], stats["answered_rps"], stats["failed"]), (50.0, 20.0, 300))


class SaturatedTest(unittest.TestCase):
    def test_failures_or_unanswered_requests_saturate_a_level(self) -> None:
        ok = {"achieved_rps": 10.0, "answered_rps": 10.0, "failed": 0, "metrics": {"throttled": 0.05}}
        self.assertIsNone(calibrate_scaler.saturated(ok, 0.2))
        self.assertIn("failed", calibrate_scaler.saturated({**ok, "failed": 3}, 0.2))
        self.assertIn("answered 8.0 of 10.0", calibrate_scaler.saturated({**ok, "answered_rps": 8.0}, 0.2))
        self.assertIn("answered 0", calibrate_scaler.saturated({**ok, "answered_rps": 0}, 0.2))

    def test_a_pod_at_its_cpu_limit_saturates_a_level_that_answers_everything(self) -> None:
        # otel-demo's frontend on a 300m limit: 10 RPS at 0.176 cores, 20 RPS at 0.272 with p95 doubled.
        def otel(rps: float, cpu: float) -> dict:
            return {"achieved_rps": rps, "answered_rps": rps, "failed": 0, "metrics": {"cpu_cores": cpu, "throttled": 0.5}}

        self.assertIsNone(calibrate_scaler.saturated(otel(10, 0.176), 0.3))
        self.assertIn("CPU at 91% of the 0.3 core limit", calibrate_scaler.saturated(otel(20, 0.272), 0.3))

    def test_hotels_levels_stay_below_the_cpu_limit_check(self) -> None:
        # hotel-reservation's calibration on a 200m limit: 0.59 of it at 20 RPS, the highest level.
        level = {"achieved_rps": 20.0, "answered_rps": 20.0, "failed": 0, "metrics": {"cpu_cores": 0.118, "throttled": 0.24}}
        self.assertIsNone(calibrate_scaler.saturated(level, 0.2))

    def test_throttling_alone_does_not_saturate_a_level(self) -> None:
        # hotel-reservation at 10 RPS on a 200m limit: every request answered, 31% CPU, 20% throttled.
        level = {"achieved_rps": 10.0, "answered_rps": 10.0, "failed": 0, "metrics": {"cpu_cores": 0.062, "throttled": 0.2}}
        self.assertIsNone(calibrate_scaler.saturated(level, 0.2))


class ParseSampleTest(unittest.TestCase):
    def test_reads_rates_and_skips_the_byte_counters(self) -> None:
        text = """croft_container_cpu_cores{container_id="abc"} 0.25
croft_container_syscall_rate{container_id="abc",syscall="writev"} 106.4
croft_container_syscall_rate{container_id="other",syscall="writev"} 9
croft_container_syscall_bytes_total{container_id="abc",syscall="writev"} 4754039
croft_container_cpu_throttled_fraction{container_id="abc"} 0.08
"""
        self.assertEqual(
            calibrate_scaler.parse_sample(text, "abc"), {"cpu_cores": 0.25, "writev": 106.4, "throttled": 0.08}
        )


if __name__ == "__main__":
    unittest.main()
