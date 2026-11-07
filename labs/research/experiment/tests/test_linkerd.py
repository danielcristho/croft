"""linkerd without a cluster: when pods need a restart, edge queries, the ingress edge, NaN quantiles and the edges.csv layout."""

import csv
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from experiment import linkerd, prometheus


def stream(metric: dict, values: list[tuple[int, str]]) -> dict:
    return {"metric": metric, "values": [[timestamp, value] for timestamp, value in values]}


def answer(query: str, start_s: int, end_s: int) -> list[dict]:
    if 'direction="inbound"' in query:
        return [stream({"deployment": "frontend"}, [(100, "20"), (105, "300")])]
    if "histogram_quantile" in query:
        return [stream({"deployment": "frontend", "dst_deployment": "search"}, [(100, "NaN"), (105, "40")])]
    return [
        stream({"deployment": "frontend", "dst_deployment": "search"}, [(100, "20"), (105, "290")]),
        stream({"deployment": "search", "dst_deployment": "geo"}, [(105, "290")]),
    ]


def pod(*containers: str, init: tuple[str, ...] = ()) -> dict:
    return {"spec": {"containers": [{"name": n} for n in containers], "initContainers": [{"name": n} for n in init]}}


class LinkerdTest(unittest.TestCase):
    def test_restart_when_a_pod_disagrees_with_the_mesh(self) -> None:
        plain, sidecar, native = pod("frontend"), pod("frontend", "linkerd-proxy"), pod("frontend", init=("linkerd-init", "linkerd-proxy"))
        self.assertTrue(linkerd.needs_restart([sidecar, plain], mesh=True))
        self.assertFalse(linkerd.needs_restart([sidecar, native], mesh=True))
        self.assertTrue(linkerd.needs_restart([native], mesh=False))
        self.assertFalse(linkerd.needs_restart([plain], mesh=False))
        self.assertFalse(linkerd.needs_restart([], mesh=True))

    def test_edge_queries_group_by_the_given_labels(self) -> None:
        queries = linkerd.edge_queries('namespace="hotel-res",direction="outbound"', ("deployment", "dst_deployment"))
        self.assertEqual(list(queries), linkerd.EDGE_COLUMNS)
        self.assertIn("sum by (deployment, dst_deployment) (rate(response_total{", queries["rps"])
        self.assertIn('classification="success"', queries["success_rate"])
        self.assertIn("sum by (le, deployment, dst_deployment)", queries["p95_ms"])
        self.assertTrue(queries["p95_ms"].startswith("histogram_quantile(0.95,"))

    def test_series_by_keeps_streams_apart_and_drops_nan(self) -> None:
        with mock.patch.object(prometheus, "query_range", answer):
            found = prometheus.series_by("histogram_quantile(0.95, x)", ("deployment", "dst_deployment"), 0, 1)
        self.assertEqual(found, {("frontend", "search"): {105: 40.0}})

    def test_series_still_sums_every_stream(self) -> None:
        with mock.patch.object(prometheus, "query_range", answer):
            self.assertEqual(prometheus.series("response_total", 0, 1), {100: 20.0, 105: 580.0})

    def test_signal_query_joins_the_app_pod_and_leaves_out_the_proxy(self) -> None:
        query = prometheus.signal_query("boutique", "m_total", ("syscall",), True)
        self.assertIn("rate(m_total[15s]) * on (container_id) group_left (pod)", query)
        self.assertIn('namespace="boutique",container!="linkerd-proxy"', query)
        self.assertIn("sum by (service, syscall)", query)
        self.assertNotIn("rate(", prometheus.signal_query("boutique", "m", (), False))

    def test_signals_csv_is_long_and_joins_multiple_labels(self) -> None:
        def result(query: str, start_s: int, end_s: int) -> list[dict]:
            if "croft_container_connects_total" not in query:
                return []
            metric = {"service": "frontend", "address": "10.0.0.1", "port": "53"}
            return [{"metric": metric, "values": [[10, "2.5"]]}]

        with tempfile.TemporaryDirectory() as directory, mock.patch.object(prometheus, "query_range", result):
            path = Path(directory) / "signals.csv"
            prometheus.export_signals(path, "boutique", 0, 20)
            with path.open(newline="") as f:
                rows = list(csv.DictReader(f))
        self.assertEqual(rows, [{"unix_s": "10", "service": "frontend", "signal": "connects_s", "key": "10.0.0.1:53", "value": "2.5"}])

    def test_edges_csv_holds_ingress_and_meshed_calls(self) -> None:
        with mock.patch.object(prometheus, "query_range", answer), tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "edges.csv"
            linkerd.export_edges(path, "hotel-res", "frontend", 0, 1)
            with path.open() as handle:
                rows = list(csv.DictReader(handle))
        self.assertEqual(list(rows[0]), ["unix_s", "src", "dst", *linkerd.EDGE_COLUMNS])
        edges = {(row["unix_s"], row["src"], row["dst"]): row for row in rows}
        self.assertEqual(edges[("105", "ingress", "frontend")]["rps"], "300.0")
        self.assertEqual(edges[("105", "search", "geo")]["rps"], "290.0")
        self.assertEqual(edges[("100", "frontend", "search")]["p95_ms"], "")
        self.assertEqual(edges[("105", "frontend", "search")]["p95_ms"], "40.0")
        self.assertEqual([row["unix_s"] for row in rows], sorted(row["unix_s"] for row in rows))


if __name__ == "__main__":
    unittest.main()
