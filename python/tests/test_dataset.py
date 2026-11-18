"""File discovery and windowed reads of croft.dataset over plain and zstd files."""

import json
import tempfile
import unittest
from pathlib import Path

import zstandard

from croft import dataset


def lines(*stamps):
    return "".join(json.dumps({"unix_ms": ms, "tgid": 1}, separators=(",", ":")) + "\n" for ms in stamps).encode()


class DatasetTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        node = self.root / "node-a"
        node.mkdir()
        (node / "croft-dataset-20261101T000000Z.jsonl.zst").write_bytes(
            zstandard.ZstdCompressor().compress(lines(1, 2))
        )
        (node / dataset.LIVE).write_bytes(lines(3, 4) + b"not json\n")
        (self.root / "node-b.jsonl").write_bytes(lines(5))

    def test_files_in_time_order_per_node(self):
        names = [(dataset.node(path), path.name) for path in dataset.files(self.root)]
        self.assertEqual(
            names,
            [
                ("node-b", "node-b.jsonl"),
                ("node-a", "croft-dataset-20261101T000000Z.jsonl.zst"),
                ("node-a", dataset.LIVE),
            ],
        )

    def test_read_window_and_unreadable(self):
        rows, unreadable = dataset.read(paths=dataset.files(self.root), since_ms=2, until_ms=4)
        self.assertEqual(
            sorted((row["node"], row["unix_ms"]) for row in rows), [("node-a", 2), ("node-a", 3), ("node-a", 4)]
        )
        self.assertEqual(unreadable, 1)


if __name__ == "__main__":
    unittest.main()
