"""scaler: CPU quantities of a Deployment's limit, and the collector a run ran beside."""

import unittest
from unittest import mock

from experiment import kube, scaler
from types import SimpleNamespace

DAEMONSET = {"spec": {"template": {"spec": {"containers": [{"image": "ghcr.io/danielcristho/croft-collector:operands"}]}}}}

APP = SimpleNamespace(
    name="hr", namespace="hotel", deployment="frontend", suffix="",
    scaler=SimpleNamespace(syscall="sendto", calls_per_request=2, cpu_per_request_ms=3.0),
)

def pod(*image_ids: str) -> dict:
    return {"status": {"containerStatuses": [{"imageID": image_id} for image_id in image_ids]}}

def scale_args(arm: str) -> list[str]:
    deployment = {"metadata": {}, "spec": {"selector": {}, "template": {"metadata": {}, "spec": {"containers": [{"name": "scaler"}]}}}}
    return scaler.retarget(deployment, APP, arm)["spec"]["template"]["spec"]["containers"][0]["args"]


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

    def test_croft_arm_leaves_scale_up_uncapped(self):
            self.assertFalse(any(arg.startswith("--max-step-up") for arg in scale_args("croft")))

    def test_step_arm_caps_scale_up_as_the_hpa(self):
            self.assertIn("--max-step-up=4", scale_args("croft-step"))


if __name__ == "__main__":
    unittest.main()
