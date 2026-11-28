"""apps.json without a cluster: services match the manifests, bad roles and mixes stop the load, and the services.csv layout."""

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

    def test_a_target_is_seen_as_its_own_deployment(self) -> None:
        app = apps.APPS["hotel-reservation"]
        rate = app.target("rate")
        self.assertEqual((rate.deployment, rate.container, rate.selector), ("rate", "rate", "app=rate"))
        self.assertEqual(rate.hpa_manifest.name, "hpa-rate.yaml")
        self.assertEqual((app.suffix, rate.suffix), ("", "-rate"))
        self.assertEqual([t.deployment for t in app.scaled(("rate",))], ["frontend", "rate"])
        with self.assertRaisesRegex(SystemExit, "search"):
            app.target("search")
    
    def test_a_target_needs_its_hpa_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = catalog(Path(tmp), targets={"search": {"container": "search"}})
            with self.assertRaisesRegex(SystemExit, "hpa-search.yaml"):
                apps.load(path)
    
    def test_every_app_has_a_mix(self) -> None:
        for app in apps.APPS.values():
            with self.subTest(app=app.name):
                self.assertGreater(len(app.endpoints(mix=True)), 1)
                self.assertEqual([e.path for e in app.endpoints()], [app.path])

    def test_with_mix_takes_the_mix_scale_and_calibration(self) -> None:
        mix = {"scale": 4, "scaler": {"syscall": "read", "calls_per_request": 2, "cpu_per_request_ms": 3},
               "endpoints": [{"name": "a", "path": "/a"}]}
        with tempfile.TemporaryDirectory() as tmp:
            app = apps.load(catalog(Path(tmp), mix=mix))["hotel-reservation"]
        mixed = app.with_mix()
        self.assertEqual((mixed.scale, mixed.scaler.syscall), (4, "read"))
        # The app itself keeps its own scale and calibration from apps.json.
        own = apps.APPS["hotel-reservation"]
        self.assertEqual((app.scale, app.scaler), (own.scale, own.scaler))

    def test_a_bad_mix_stops_the_load(self) -> None:
        bad = {
            "duplicate": [{"name": "a", "path": "/a"}, {"name": "a", "path": "/b"}],
            "weight": [{"name": "a", "path": "/a", "weight": -1}],
            "method": [{"name": "a", "path": "/a", "method": "PUT"}],
            "get body": [{"name": "a", "path": "/a", "body": "x=1"}],
            "path": [{"name": "a", "path": "a"}],
            "all weights 0": [{"name": "a", "path": "/a", "weight": 0}],
            "unknown before": [{"name": "a", "path": "/a", "before": "b"}],
            "chained before": [{"name": "a", "path": "/a", "before": "b"}, {"name": "b", "path": "/b", "before": "c"},
                               {"name": "c", "path": "/c"}],
        }
        for case, endpoints in bad.items():
            with self.subTest(case=case), tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(SystemExit):
                    apps.load(catalog(Path(tmp), mix={"endpoints": endpoints}))

    def test_mix_users_default_to_one_and_must_be_whole(self) -> None:
        endpoints = [{"name": "a", "path": "/a"}]
        with tempfile.TemporaryDirectory() as tmp:
            app = apps.load(catalog(Path(tmp), mix={"endpoints": endpoints}))["hotel-reservation"]
        self.assertEqual((app.users(mix=True), app.users(mix=False)), (1, 1))
        for users in (0, 2.5, "50"):
            with self.subTest(users=users), tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(SystemExit):
                    apps.load(catalog(Path(tmp), mix={"endpoints": endpoints, "users": users}))

    def test_session_and_cart_mixes_spread_over_users(self) -> None:
        for name in ("online-boutique", "otel-demo"):
            app = apps.APPS[name]
            sent = " ".join(e.path + e.body + "".join(e.headers.values()) for e in app.endpoints(mix=True))
            with self.subTest(app=name):
                self.assertGreater(app.users(mix=True), 1)
                self.assertIn("croft-{user}", sent)
                self.assertNotIn("croft-load", sent)

    def test_no_mix_stops_a_mix_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            app = apps.load(catalog(Path(tmp), mix=None))["hotel-reservation"]
        with self.assertRaisesRegex(SystemExit, "no mix"):
            app.with_mix()

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
