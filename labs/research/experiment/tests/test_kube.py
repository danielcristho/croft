"""kube.nodes_without on synthetic node lists: a full image list cannot prove an image missing."""

import unittest

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


if __name__ == "__main__":
    unittest.main()
