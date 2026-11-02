"""keda: CPU quantities, the per-pod threshold and the ScaledObject it applies."""

import unittest

from experiment import keda
from experiment.apps import APPS


class KedaTest(unittest.TestCase):
    def test_cpu_quantities(self) -> None:
        self.assertEqual(keda.cpu_cores("200m"), 0.2)
        self.assertEqual(keda.cpu_cores("1"), 1.0)
        self.assertEqual(keda.cpu_cores("1.5"), 1.5)

    def test_threshold_matches_the_capacity_croft_assumes(self) -> None:
        # 0.7 x 1000 ms / 50 ms per request, and 0.7 x 200 ms / 1.6 ms.
        self.assertEqual(keda.threshold(APPS["hotel-reservation"], 0.2), 87.5)

    def test_scaled_object_counts_the_apps_ingress_requests(self) -> None:
        obj = keda.scaled_object(APPS["hotel-reservation"], 0.2)
        spec = obj["spec"]
        self.assertEqual((obj["metadata"]["name"], obj["metadata"]["namespace"]), ("frontend", "hotel-res"))
        self.assertEqual((spec["minReplicaCount"], spec["maxReplicaCount"]), (1, 10))
        trigger = spec["triggers"][0]["metadata"]
        self.assertIn('exported_namespace="hotel-res",service="frontend"', trigger["query"])
        self.assertEqual(trigger["threshold"], "87.5")


if __name__ == "__main__":
    unittest.main()
