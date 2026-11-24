"""scaler: CPU quantities of a Deployment's limit, and the collector a run ran beside."""

import unittest
from unittest import mock

from experiment import kube, scaler

DAEMONSET = {"spec": {"template": {"spec": {"containers": [{"image": "ghcr.io/danielcristho/croft-collector:operands"}]}}}}


def pod(*image_ids: str) -> dict:
    return {"status": {"containerStatuses": [{"imageID": image_id} for image_id in image_ids]}}


class ScalerTest(unittest.TestCase):
    def test_cpu_quantities(self):
        self.assertEqual(scaler.cpu_cores("200m"), 0.2)
        self.assertEqual(scaler.cpu_cores("1"), 1.0)
        self.assertEqual(scaler.cpu_cores("1.5"), 1.5)

    def test_collector_image_keeps_the_tag_and_each_digest_once(self):
        pods = [pod("ghcr.io/x@sha256:b"), pod("ghcr.io/x@sha256:a"), pod("ghcr.io/x@sha256:b"), {"status": {}}]
        self.assertEqual(scaler.collector_image(DAEMONSET, pods), {
            "image": "ghcr.io/danielcristho/croft-collector:operands",
            "image_ids": ["ghcr.io/x@sha256:a", "ghcr.io/x@sha256:b"],
        })

    def test_a_pod_not_started_yet_has_no_digest(self):
        self.assertEqual(scaler.collector_image(DAEMONSET, [pod("")])["image_ids"], [])

    def test_no_collector_deployed_records_none(self):
        missing = kube.KubectlError(1, ["kubectl"], "", 'daemonsets.apps "croft-collector" not found')
        with mock.patch.object(kube, "get", side_effect=missing):
            self.assertIsNone(scaler.collector())


if __name__ == "__main__":
    unittest.main()
