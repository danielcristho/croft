"""calibrate_scaler: picking the syscall whose calls per request hold across load levels."""

import unittest

from experiment import calibrate_scaler


def level(rps: float, cpu: float, write: float, futex: float, close: float) -> dict:
    return {"achieved_rps": rps, "metrics": {"cpu_cores": cpu, "throttled": 0.0, "write": write, "futex": futex, "close": close}}


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


if __name__ == "__main__":
    unittest.main()
