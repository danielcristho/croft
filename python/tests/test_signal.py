"""dataset.signal over recorded rows: polls, idle containers, cgroup values, names and segment choice."""

import json
import tempfile
import unittest
from pathlib import Path

from croft import dataset, kube

A, B = "aa11", "bb22"


def row(ms, container, syscalls=(), runqueue=None, cores=None):
    record = {
        "unix_ms": ms,
        "tgid": 1,
        "process": {"container_id": container},
        "usage": {"runqueue_s": runqueue},
        "syscalls": [{"name": name, "rate_per_sec": rate} for name, rate in syscalls],
    }
    if cores is not None:
        record["cgroup_usage"] = {"cpu_cores": cores}
    return json.dumps(record, separators=(",", ":"))


class SignalTest(unittest.TestCase):
    def setUp(self):
        node = Path(tempfile.mkdtemp()) / "node-a"
        node.mkdir()
        lines = [
            # poll 1: two processes of A, one of B
            row(10_000, A, [("sendto", 3.0), ("read", 1.0)], runqueue=0.5, cores=0.2),
            row(10_001, A, [("sendto", 1.0)], runqueue=0.5, cores=0.2),
            row(10_002, B, [("sendto", 8.0)], cores=1.0),
            # poll 2: B alone, A idle and not written
            row(12_000, B, [("sendto", 4.0)], cores=0.6),
        ]
        (node / dataset.LIVE).write_text("\n".join(lines) + "\n")
        self.paths = dataset.files(node.parent)

    def frame(self, name, **options):
        return dataset.signal(name, since=0, until=100, step_s=10, paths=self.paths, **options)

    def test_summed_signal_counts_a_missing_poll_as_zero(self):
        frame = self.frame("syscall_rate", match={"syscall": "sendto"})
        values = dict(zip(frame["container_id"], frame["value"]))
        self.assertEqual(values, {A: 2.0, B: 6.0})
        self.assertEqual(list(frame.columns), ["time", "node", "container_id", "syscall", "value"])

    def test_cgroup_signal_averages_the_polls_that_hold_it(self):
        frame = self.frame("cpu_cores")
        values = dict(zip(frame["container_id"], frame["value"].round(3)))
        self.assertEqual(values, {A: 0.2, B: 0.8})

    def test_names_label_and_narrow_the_rows(self):
        names = {
            A: ("boutique", "frontend-7d9f8c6b5-x2k4p", "server"),
            B: ("boutique", "cart-5c8f9d7b4-abcde", "server"),
        }
        frame = self.frame("runqueue_wait", namespace="boutique", deployment="frontend", names=names)
        self.assertEqual(
            frame[["deployment", "pod", "value"]].values.tolist(), [["frontend", "frontend-7d9f8c6b5-x2k4p", 0.5]]
        )

    def test_operand_signals_are_refused(self):
        with self.assertRaisesRegex(ValueError, "Prometheus only"):
            self.frame("connects")

    def test_namespace_without_names_is_refused(self):
        with self.assertRaisesRegex(ValueError, "need names"):
            self.frame("cpu_cores", namespace="boutique")


class BetweenTest(unittest.TestCase):
    def test_segments_outside_the_window_are_skipped(self):
        node = Path("/data/node-a")
        paths = [
            node / "croft-dataset-20261101T000000Z.jsonl.zst",
            node / "croft-dataset-20261102T000000Z.jsonl.zst",
            node / dataset.LIVE,
        ]
        start = 1_793_620_800_000  # 2026-11-02 12:00 UTC
        self.assertEqual(dataset.between(paths, start, start + 3_600_000), paths[1:])
        self.assertEqual(dataset.between(paths, 0, 1_000), paths[2:])


class NamesTest(unittest.TestCase):
    def test_current_and_previous_containers(self):
        pods = {
            "items": [
                {
                    "metadata": {"namespace": "boutique", "name": "frontend-7d9f8c6b5-x2k4p"},
                    "status": {
                        "containerStatuses": [
                            {
                                "name": "server",
                                "containerID": "containerd://aa11",
                                "lastState": {"terminated": {"containerID": "containerd://cc33"}},
                            }
                        ]
                    },
                }
            ]
        }
        found = kube.names_of(pods)
        self.assertEqual(found["aa11"], ("boutique", "frontend-7d9f8c6b5-x2k4p", "server"))
        self.assertEqual(found["cc33"], found["aa11"])


if __name__ == "__main__":
    unittest.main()
