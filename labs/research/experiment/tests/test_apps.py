"""apps.json without a cluster: services match the manifests, bad roles stop the load, and the services.csv layout."""

import csv
import json
import re
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from experiment import apps, kube, prometheus


def deployments(manifest: Path) -> set[str]:
    documents = re.split(r"^---", manifest.read_text(), flags=re.M)
    return {
        re.search(r"^metadata:\n(?:  .*\n)*?  name:\s*(\S+)", doc, re.M).group(1)
        for doc in documents
        if re.search(r"^kind:\s*Deployment\b", doc, re.M)
    }


def catalog(directory: Path, **changes: object) -> Path:
    entry = json.loads(apps.CATALOG.read_text())["hotel-reservation"]
    entry["manifests"] = str(apps.ROOT / entry["manifests"])
    entry.update(changes)
    path = directory / "apps.json"
    path.write_text(json.dumps({"hotel-reservation": entry}))
    return path


class AppsTest(unittest.TestCase):
    def test_services_list_every_deployment_of_the_manifest(self) -> None:
        for app in apps.APPS.values():
            with self.subTest(app=app.name):
                self.assertEqual(set(app.services), deployments(app.manifests / "k8s.yaml"))

    def test_an_unknown_role_stops_the_load(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = catalog(Path(tmp), services={"frontend": {"role": "edge", "language": "go"}})
            with self.assertRaisesRegex(SystemExit, "edge"):
                apps.load(path)

    def test_the_scaled_deployment_must_be_a_service(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = catalog(Path(tmp), services={"search": {"role": "business", "language": "go"}})
            with self.assertRaisesRegex(SystemExit, "frontend"):
                apps.load(path)

    def test_services_csv_labels_known_services_and_leaves_others_empty(self) -> None:
        app = apps.APPS["hotel-reservation"]
        listed = {"items": [{"metadata": {"name": "frontend"}}, {"metadata": {"name": "debug"}}]}

        def result(query: str, start_s: int, end_s: int) -> list[dict]:
            return [{"metric": {}, "values": [[10, "0.5"]]}] if "container_cpu_usage_seconds_total" in query else []

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(kube, "get", return_value=listed), \
                mock.patch.object(prometheus, "query_range", result):
            path = Path(tmp) / "services.csv"
            prometheus.export_services(path, app, 0, 20)
            with path.open(newline="") as f:
                rows = list(csv.DictReader(f))
        self.assertEqual([(r["service"], r["role"], r["language"], r["cpu_cores"]) for r in rows],
                         [("debug", "", "", "0.5"), ("frontend", "gateway", "go", "0.5")])


if __name__ == "__main__":
    unittest.main()
