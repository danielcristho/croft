"""kube.nodes_without: a full image list cannot prove an image miissing. Kubectl carry their stderr."""
import subprocess
import unittest
from unittest import mock

from experiment import kube

IMAGE = "registry.example/frontend:v1"


def node(name: str, images: list[str], ready: bool = True) -> dict:
    return {
        "metadata": {"name": name},
        "status": {
            "conditions": [{"type": "Ready", "status": "True" if ready else "False"}],
            "images": [{"names": [image], "sizeBytes": 1} for image in images],
        },
    }


class NodesWithoutTest(unittest.TestCase):
    def test_a_short_list_without_the_image_is_missing(self) -> None:
        self.assertEqual(kube.nodes_without(IMAGE, [node("a", ["other:1"])]), (["a"], []))

    def test_a_full_list_without_the_image_is_unknown(self) -> None:
        full = [f"filler:{i}" for i in range(kube.REPORTED_IMAGES)]
        self.assertEqual(kube.nodes_without(IMAGE, [node("a", full)]), ([], ["a"]))

    def test_holding_nodes_and_unready_nodes_are_skipped(self) -> None:
        nodes = [node("a", [IMAGE]), node("b", ["other:1"], ready=False), node("c", ["docker.io/" + IMAGE])]
        self.assertEqual(kube.nodes_without(IMAGE, nodes), ([], []))

class KubectlTest(unittest.TestCase):
    def test_a_failed_call_shows_its_stderr(self) -> None:
        stderr = "error: no configuration has been provided\n"
        failed = subprocess.CompletedProcess(["kubectl", "get", "pods"], 1, "", stderr)
        with mock.patch.object(kube.subprocess, "run", return_value=failed):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                kube.kubectl("get", "pods")
        self.assertIn("exit status 1", str(raised.exception))
        self.assertIn("error: no configuration has been provided", str(raised.exception))

    def test_a_successful_call_returns_stdout(self) -> None:
        done = subprocess.CompletedProcess(["kubectl", "version"], 0, "v1.34\n", "")
        with mock.patch.object(kube.subprocess, "run", return_value=done):
            self.assertEqual(kube.kubectl("version"), "v1.34\n")

if __name__ == "__main__":
    unittest.main()
