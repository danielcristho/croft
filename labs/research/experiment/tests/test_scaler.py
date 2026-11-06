"""scaler: CPU quantities of a Deployment's limit."""

import unittest

from experiment import scaler


class ScalerTest(unittest.TestCase):
    def test_cpu_quantities(self):
        self.assertEqual(scaler.cpu_cores("200m"), 0.2)
        self.assertEqual(scaler.cpu_cores("1"), 1.0)
        self.assertEqual(scaler.cpu_cores("1.5"), 1.5)


if __name__ == "__main__":
    unittest.main()
