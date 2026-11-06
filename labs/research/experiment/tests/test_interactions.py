"""interactions without a cluster: the p95 bins of a capacity ramp, the knee read from them, and the run summary."""

import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from experiment import interactions


def ramp(knee_rps: float) -> list[dict]:
    rates = [10 + 190 * i / 600 for i in range(600)]
    return [{"target_rps": str(r), "p95_latency_ms": str(50 if r < knee_rps else 50 + (r - knee_rps) * 20)} for r in rates]


class Capacity(unittest.TestCase):
    def test_knee_where_p95_leaves_its_floor(self):
        self.assertEqual(interactions.knee(interactions.p95_by_rate(ramp(80))), 80)

    def test_no_knee_on_a_flat_ramp(self):
        self.assertIsNone(interactions.knee(interactions.p95_by_rate(ramp(1000))))

    def test_unanswered_seconds_are_left_out(self):
        rows = [{"target_rps": "15", "p95_latency_ms": "0"}, {"target_rps": "16", "p95_latency_ms": "40"}]
        self.assertEqual(interactions.p95_by_rate(rows), [(10, 40.0)])

    def test_one_spike_is_not_a_knee(self):
        table = [(10, 50.0), (20, 50.0), (30, 50.0), (40, 300.0), (50, 50.0), (60, 50.0)]
        self.assertIsNone(interactions.knee(table))


class Summary(unittest.TestCase):
    def test_concurrent_column(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(interactions, "WEB", Path(directory)):
            lines = [{"run_id": "si-otel-demo-lo-1-burst", "mesh": "linkerd", "concurrent": True},
                     {"run_id": "si-otel-demo-hi-1-burst", "mesh": "linkerd"}, {"run_id": "otel-try-none-burst"}]
            (Path(directory) / "runs.jsonl").write_text("".join(json.dumps(line) + "\n" for line in lines))
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                interactions.summary()
        rows = {line.split()[0]: line.split() for line in out.getvalue().splitlines() if line.startswith("si-")}
        self.assertEqual(rows["si-otel-demo-lo-1-burst"][2], "True")
        self.assertEqual(rows["si-otel-demo-hi-1-burst"][2], "False")
        self.assertIn("2 si-* runs", out.getvalue())


if __name__ == "__main__":
    unittest.main()
