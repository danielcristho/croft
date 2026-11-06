"""compare on a synthetic burst run: log and event parsing, the burst window, scores and arm summaries."""

import csv
import json
import tempfile
import unittest
from pathlib import Path

from experiment import compare, scaler
from experiment.apps import APPS

STARTED_MS = 1_800_000_000_000
BURST_MS = STARTED_MS + 180_000
END_MS = BURST_MS + 120_000


def record(run_id: str, scaler_name: str | None) -> dict:
    return {
        "run_id": run_id,
        "load": "open-loop",
        "profile": {"name": "burst", "burst_start_s": 180.0, "burst_duration_s": 120.0},
        "scale": 0.5,
        "probes": "relaxed",
        "scaler": scaler_name,
        "started_ms": STARTED_MS,
        "directory": f"data/web/{run_id}",
    }


def stamp(ms: int) -> str:
    return f"2027-01-15T08:{(ms // 1000 // 60) % 60:02d}:{(ms // 1000) % 60:02d}.123456789Z"


def write_csv(path: Path, rows: list[dict]) -> None:
    with path.open("w", newline="") as out:
        writer = csv.DictWriter(out, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def write_run(directory: Path, with_log: bool) -> None:
    directory.mkdir(parents=True)
    workload = [
        {"timestamp_epoch": (BURST_MS + s * 1000) / 1000, "target_rps": 75, "success": 60, "failed": 15,
         "p95_latency_ms": 900 if s <= 10 else 100}
        for s in range(1, 121)
    ]
    workload.insert(0, {"timestamp_epoch": BURST_MS / 1000 - 1, "target_rps": 5, "success": 5, "failed": 0, "p95_latency_ms": 2000})
    write_csv(directory / "workload.csv", workload)
    write_csv(directory / "prometheus.csv", [
        {"unix_s": STARTED_MS // 1000, "replicas": 1},
        {"unix_s": BURST_MS // 1000 + 5, "replicas": 2},
        {"unix_s": BURST_MS // 1000 + 30, "replicas": 6},
        {"unix_s": BURST_MS // 1000 + 60, "replicas": ""},
        {"unix_s": BURST_MS // 1000 + 90, "replicas": 4},
    ])
    (directory / "timeline.json").write_text(json.dumps({
        "rescales": [
            {"first_ms": BURST_MS + 25_000, "last_ms": BURST_MS + 25_000, "count": 1,
             "message": "New size: 6; reason: cpu resource utilization (percentage of request) above target"},
            {"first_ms": BURST_MS + 80_000, "last_ms": BURST_MS + 80_000, "count": 1,
             "message": "New size: 4; reason: All metrics below target"},
        ],
        "replica_sets": [
            {"first_ms": BURST_MS + 4_000, "last_ms": BURST_MS + 4_000, "count": 1, "message": "Scaled up replica set frontend-abc from 1 to 2"},
            {"first_ms": BURST_MS + 80_000, "last_ms": BURST_MS + 80_000, "count": 1, "message": "Scaled down replica set frontend-abc from 6 to 4"},
            {"first_ms": END_MS + 60_000, "last_ms": END_MS + 60_000, "count": 1, "message": "Scaled down replica set frontend-abc from 4 to 1"},
        ],
        "pods": [
            {"pod": "frontend-a", "created_ms": STARTED_MS - 60_000, "ready_ms": STARTED_MS - 50_000},
            {"pod": "frontend-b", "created_ms": BURST_MS + 4_000, "ready_ms": BURST_MS + 12_000},
            {"pod": "frontend-c", "created_ms": BURST_MS + 25_000, "ready_ms": None},
        ],
    }))
    if with_log:
        (directory / "scaler.log").write_text(
            f"{stamp(BURST_MS - 10_000)} [INFO  croft::command::scale] scaling hotel-res/frontend from write per request\n"
            f"{stamp(BURST_MS + 2_000)} [DEBUG croft::command::scale] rate=20.1/s needed=2 replicas=1\n"
            f"{stamp(BURST_MS + 4_000)} \x1b[2;90m08:03:04.123\x1b[0m INFO \x1b[0m \x1b[90mcommand::scale\x1b[0m scale up "
            "\x1b[90mfrom=\x1b[0m\x1b[97m1\x1b[0m \x1b[90mto=\x1b[0m\x1b[97m2\x1b[0m \x1b[90mrate=\x1b[0m\x1b[97m20.1/s\x1b[0m\n"
        )


class CompareTest(unittest.TestCase):
    def setUp(self) -> None:
        self.root = Path(tempfile.mkdtemp())

    def test_burst_window_follows_the_profile(self) -> None:
        self.assertEqual(compare.burst_window(record("r", None)), (BURST_MS, END_MS))

    def test_scaler_log_keeps_scale_ups_with_their_kubectl_time(self) -> None:
        write_run(self.root / "run", with_log=True)
        decisions = compare.parse_scaler_log((self.root / "run" / "scaler.log").read_text())
        self.assertEqual(len(decisions), 1)
        self.assertEqual(decisions[0][1], 2)

    def test_scaler_log_separates_scale_downs(self) -> None:
        log = ("2026-11-02T00:00:01Z INFO scale up from=1 to=5 rate=9.0/s\n"
               "2026-11-02T00:05:01Z INFO scale down from=5 to=1 rate=1.0/s\n"
               "2026-11-02T00:06:01Z INFO scale up from=1 to=3 rate=5.0/s\n")
        self.assertEqual([n for _, n in compare.parse_scaler_log(log)], [5, 3])
        both = compare.parse_scaler_log(log, ("up", "down"))
        self.assertEqual([n for _, n in both], [5, 1, 3])
        steps = compare.replica_steps({"started_ms": 0}, [], both)
        self.assertEqual(compare.decision_metrics(steps, 0, 1)["pods_created"], 6)

    def test_hpa_rescales_count_only_scale_ups(self) -> None:
        rescales = [
            {"first_ms": 1, "message": "New size: 6; reason: cpu resource utilization (percentage of request) above target"},
            {"first_ms": 2, "message": "New size: 4; reason: All metrics below target"},
        ]
        self.assertEqual(compare.parse_rescales(rescales), [(1, 6)])

    def test_run_scores_its_burst_window(self) -> None:
        write_run(self.root / "run", with_log=False)
        scores = compare.run_metrics(record("run", None), self.root / "run")
        self.assertEqual(scores["scaler"], "hpa")
        self.assertEqual(scores["hpa_first_up_s"], 25.0)
        self.assertIsNone(scores["croft_first_up_s"])
        self.assertEqual(scores["first_ready_s"], 12.0)
        self.assertEqual(scores["peak_replicas"], 6)
        self.assertEqual(scores["time_to_peak_s"], 25.0)
        self.assertEqual(scores["scale_down_events"], 1)
        self.assertEqual(scores["pods_created"], 5)
        self.assertEqual(scores["slo_violation_s"], 10)
        self.assertEqual(scores["failed_fraction"], 0.2)
        self.assertEqual(scores["success_ratio"], 0.8)
        self.assertEqual(scores["replica_seconds"], 13 * 5)

    def test_replicas_follow_every_decision_the_scrape_misses(self) -> None:
        # croft asks for 5 twice and the HPA cuts back to 4 three times, merged into one event.
        croft = [(BURST_MS + 5_000, 2), (BURST_MS + 20_000, 5), (BURST_MS + 40_000, 5)]
        rescales = [{"first_ms": BURST_MS + 30_000, "last_ms": BURST_MS + 60_000, "count": 3,
                     "message": "New size: 4; reason: All metrics below target"}]
        steps = compare.replica_steps(record("r", "croft"), rescales, croft)
        self.assertEqual([n for _, n in steps], [1, 2, 5, 4, 5, 4, 4])
        scores = compare.decision_metrics(steps, BURST_MS, END_MS)
        self.assertEqual(scores["peak_replicas"], 5)
        self.assertEqual(scores["time_to_peak_s"], 20.0)
        self.assertEqual(scores["pods_created"], 5)

    def test_scoring_one_series_keeps_the_others(self) -> None:
        previous = [{"run_id": "hr-1", "peak_replicas": "3"}, {"run_id": "web-1", "peak_replicas": "7"}]
        merged = compare.merge(previous, [{"run_id": "web-1", "peak_replicas": 8}, {"run_id": "otel-1", "peak_replicas": 4}])
        self.assertEqual([(row["run_id"], row["peak_replicas"]) for row in merged],
                         [("hr-1", "3"), ("web-1", 8), ("otel-1", 4)])

    def test_summary_groups_runs_by_arm(self) -> None:
        runs = [
            {**compare.arm(record("hpa-1", None)), "slo_violation_s": 10, "croft_first_up_s": None},
            {**compare.arm(record("hpa-2", None)), "slo_violation_s": 20, "croft_first_up_s": None},
            {**compare.arm(record("duo-1", "croft")), "slo_violation_s": 4, "croft_first_up_s": 2.0},
            {**compare.arm(record("duo-2", "croft")), "slo_violation_s": 6, "croft_first_up_s": None},
        ]
        runs.append({**compare.arm({**record("hr-1", None), "app": "hotel-reservation"}), "slo_violation_s": 3})
        hpa, croft, hotel = compare.summarize(runs)
        self.assertEqual((hotel["app"], hotel["n"]), ("hotel-reservation", 1))
        self.assertEqual((hpa["scaler"], hpa["n"], hpa["slo_violation_s"]), ("hpa", 2, "15.00 ± 7.07"))
        self.assertEqual(hpa["croft_first_up_s"], "-")
        self.assertEqual(croft["croft_first_up_s"], "2.00 ± 0.00 [1]")

    def test_reruns_keep_their_last_record_and_other_loads_are_skipped(self) -> None:
        runs = self.root / "runs.jsonl"
        steps = {"run_id": "lead-steps", "users": 12}
        runs.write_text("\n".join(json.dumps(r) for r in [record("duo", None), steps, record("duo", "croft")]))
        self.assertEqual([r["scaler"] for r in compare.burst_records(runs, [])], ["croft"])
        with self.assertRaises(SystemExit):
            compare.burst_records(runs, ["other"])
        with self.assertRaises(SystemExit):
            compare.burst_records(self.root / "missing.jsonl", [])


class ScalerTest(unittest.TestCase):
    def test_retarget_sets_calibration_and_debug_log(self) -> None:
        deployment = {
            "metadata": {"name": "croft-scaler-hotel-reservation"},
            "spec": {"selector": {}, "template": {"metadata": {}, "spec": {"containers": [
                {"name": "scaler", "args": [], "env": [{"name": "RUST_LOG", "value": "info"}]},
                {"name": "proxy"},
            ]}}},
        }
        container = scaler.retarget(deployment, APPS["hotel-reservation"])["spec"]["template"]["spec"]["containers"][0]
        self.assertIn("--calls-per-request=11.4", container["args"])
        self.assertEqual(container["env"], [{"name": "RUST_LOG", "value": scaler.LOG_LEVEL}])


if __name__ == "__main__":
    unittest.main()
