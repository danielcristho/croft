"""records without a cluster: the whole-cluster lock against the per-app locks of --concurrent runs."""

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from experiment import records


class Locks(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        data = Path(self.directory.name)
        patches = [mock.patch.object(records, "DATA", data), mock.patch.object(records, "LOCK", data / "experiment.lock")]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.held = []
        self.addCleanup(self.release)

    def release(self):
        for handle in self.held:
            handle.close()
        self.directory.cleanup()

    def hold(self, app=None):
        self.held += records.hold_lock(app)

    def test_runs_of_different_apps_share_the_cluster(self):
        self.hold("hotel-reservation")
        self.hold("otel-demo")
        self.assertEqual(len(self.held), 4)

    def test_two_runs_of_one_app_are_refused(self):
        self.hold("otel-demo")
        with self.assertRaises(SystemExit):
            self.hold("otel-demo")

    def test_a_whole_cluster_run_waits_for_concurrent_runs(self):
        self.hold("otel-demo")
        with self.assertRaises(SystemExit):
            self.hold()

    def test_concurrent_runs_wait_for_a_whole_cluster_run(self):
        self.hold()
        with self.assertRaises(SystemExit):
            self.hold("otel-demo")

    def test_a_refused_lock_holds_nothing(self):
        self.hold()
        with self.assertRaises(SystemExit):
            self.hold("otel-demo")
        self.held.pop().close()
        self.hold("otel-demo")


if __name__ == "__main__":
    unittest.main()
