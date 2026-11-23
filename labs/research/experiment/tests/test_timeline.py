"""Pods in timeline.json without a cluster: Prometheus's pod times, and merging them with the pods still running."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from experiment import compare, prometheus, records, web
from experiment.apps import APPS

LIVE = {"pod": "frontend-7d4b768894-h6ptc", "node": "digi-flood", "created_ms": 1_000_000, "ready_ms": 1_002_000,
        "container_ids": ["fad9"]}


def stream(pod: str, values: list[str]) -> dict:
    return {"metric": {"pod": pod}, "values": [[i, value] for i, value in enumerate(values)]}


class PodTimesTest(unittest.TestCase):
    def test_reads_creation_and_first_ready_per_pod(self) -> None:
        def result(query: str, start_s: int, end_s: int) -> list[dict]:
            if query.startswith("kube_pod_created"):
                return [stream("frontend-a1b2c3d4e-zmhj7", ["1100.0", "1100.0"])]
            # Ready again after a failed probe: the first Ready counts.
            return [stream("frontend-a1b2c3d4e-zmhj7", ["1102.5", "1150.0"]), stream("frontend-a1b2c3d4e-q8r9s", ["NaN"])]

        with mock.patch.object(prometheus, "query_range", result):
            times = prometheus.pod_times("hotel-res", "frontend", 1000, 1300)
        self.assertEqual(times, {"frontend-a1b2c3d4e-zmhj7": {"created_ms": 1_100_000, "ready_ms": 1_102_500}})

    def test_the_regex_keeps_a_deployment_with_a_longer_name_out(self) -> None:
        import re
        pattern = re.compile(prometheus.pod_regex("frontend"))
        self.assertTrue(pattern.fullmatch("frontend-7d4b768894-h6ptc"))
        self.assertFalse(pattern.fullmatch("frontend-proxy-6b5c8d9f7-abcde"))


class MergePodsTest(unittest.TestCase):
    def test_adds_the_pods_a_scale_down_deleted(self) -> None:
        recorded = {
            LIVE["pod"]: {"created_ms": 1_000_000, "ready_ms": 1_002_000},
            "frontend-7d4b768894-z7r7d": {"created_ms": 1_235_000, "ready_ms": 1_236_000},
            "frontend-7d4b768894-new00": {"created_ms": 1_240_000},
        }
        pods = web.merge_pods([LIVE], recorded, since_ms=900_000)
        self.assertEqual([p["pod"] for p in pods], [LIVE["pod"], "frontend-7d4b768894-z7r7d", "frontend-7d4b768894-new00"])
        self.assertEqual(pods[0]["container_ids"], ["fad9"])
        self.assertEqual((pods[1]["ready_ms"], pods[1]["container_ids"]), (1_236_000, []))
        self.assertIsNone(pods[2]["ready_ms"])

    def test_leaves_out_pods_of_an_earlier_deploy(self) -> None:
        pods = web.merge_pods([], {"frontend-0ld0ld0ld-aaaaa": {"created_ms": 500_000, "ready_ms": 501_000}}, since_ms=900_000)
        self.assertEqual(pods, [])

    def test_fills_a_live_pod_that_was_not_ready_yet(self) -> None:
        pending = {**LIVE, "ready_ms": None}
        pods = web.merge_pods([pending], {LIVE["pod"]: {"created_ms": 1_000_000, "ready_ms": 1_003_000}}, since_ms=900_000)
        self.assertEqual(pods[0]["ready_ms"], 1_003_000)

    def test_first_ready_counts_a_deleted_pod(self) -> None:
        # The pods of f2-hr-croft-1: one from the deploy, then one croft added 12.2 s into the burst.
        record = {"run_id": "r", "started_ms": 1_000_000, "profile": {"burst_start_s": 180, "burst_duration_s": 120}}
        burst_ms = compare.burst_window(record)[0]
        recorded = {"frontend-7d4b768894-z7r7d": {"created_ms": burst_ms + 12_200, "ready_ms": burst_ms + 13_200}}
        pods = web.merge_pods([LIVE], recorded, since_ms=900_000)
        ready = compare.seconds_after(burst_ms, (p["ready_ms"] for p in pods if p["created_ms"] >= burst_ms and p["ready_ms"]))
        self.assertEqual(ready, 13.2)


class BackfillTest(unittest.TestCase):
    def test_rewrites_the_timeline_with_the_missing_pods(self) -> None:
        record = {"run_id": "f2-hr-croft-1-burst", "started_ms": 1_040_000, "ended_ms": 1_940_000}
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(records, "DATA", Path(tmp)):
            directory = Path(tmp) / "web" / record["run_id"]
            directory.mkdir(parents=True)
            (directory / "timeline.json").write_text(json.dumps({"rescales": [], "replica_sets": [], "pods": [LIVE]}))
            added = {"frontend-7d4b768894-z7r7d": {"created_ms": 1_235_000, "ready_ms": 1_236_000}}
            with mock.patch.object(prometheus, "pod_times", return_value=added) as times:
                self.assertEqual(web.backfill_pods(APPS["hotel-reservation"], record), (1, 2))
            # The earliest pod already recorded bounds the query, not the run's start.
            self.assertEqual(times.call_args.args, ("hotel-res", "frontend", 1000, 1941))
            found = json.loads((directory / "timeline.json").read_text())
        self.assertEqual([p["pod"] for p in found["pods"]], [LIVE["pod"], "frontend-7d4b768894-z7r7d"])
        self.assertEqual(found["rescales"], [])


if __name__ == "__main__":
    unittest.main()
